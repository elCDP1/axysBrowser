//! Web-load helpers shared by the window's navigation paths.
//!
//! Extracted from `ui::window` so the main window file stays focused on
//! wiring: address-bar text rules and the hung-load watchdog live here.

use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::browser::address::{SearchConfig, resolve_input};
use crate::browser::engine::BrowserEngine;
use crate::browser::tabs::Tab;
use crate::internal::router;
use webkit6::WebView;
use webkit6::prelude::WebViewExt;

/// Seconds without committing a load before the watchdog gives up on it.
pub const LOAD_TIMEOUT_SECS: u64 = 60;

/// Resolves address-bar or startup input to a tab target: typed text becomes
/// a URL or a search, and an unknown `axys://` page falls back to New Tab
/// instead of being silently dropped (no feedback at all).
pub fn resolve_target(input: &str, config: &SearchConfig) -> String {
    let target = resolve_input(input, config);

    if target.starts_with("axys://") && router::page_name(&target).is_none() {
        "axys://newtab".to_string()
    } else {
        target
    }
}

/// Whether a resolved target can be shown in a tab. Everything else (mail
/// clients, dialers, …) is handed to the desktop by the policy handler and
/// must not become an empty web tab.
pub fn is_tab_target(target: &str) -> bool {
    router::page_name(target).is_some()
        || target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("file://")
        || target == "about:blank"
}

/// Address bar text for a tab URI: the new tab and privacy pages show an
/// empty bar like Chromium's NTP; every other page keeps its URL.
pub fn address_text_for(uri: &str) -> &str {
    match router::page_name(uri) {
        Some("newtab") | Some("privacy") => "",
        _ => uri,
    }
}

/// Marks a fresh web load on `id` and arms the hang watchdog: if nothing
/// has committed after [`LOAD_TIMEOUT_SECS`] (DNS/connect/TLS stuck), the
/// load is stopped and the error page is shown instead of spinning
/// forever. A page that already committed is never replaced (slow
/// subresources are normal), and a watchdog from an older navigation
/// (different serial) never kills a newer load. The serial, not the URI,
/// identifies the load: redirects change the URI while the load is still
/// the same one.
pub fn begin_web_load(tabs: &Rc<RefCell<Vec<Tab>>>, id: usize, web_view: &WebView, target: &str) {
    let serial = {
        let mut tabs = tabs.borrow_mut();

        tabs.iter_mut()
            .find(|tab| tab.id == id)
            .map(|tab| {
                tab.nav_serial += 1;
                tab.committed = false;

                tab.nav_serial
            })
            .unwrap_or(0)
    };

    let weak = web_view.downgrade();
    let tabs = Rc::downgrade(tabs);
    let target = target.to_string();

    gtk::glib::timeout_add_local_once(
        std::time::Duration::from_secs(LOAD_TIMEOUT_SECS),
        move || {
            let Some(view) = weak.upgrade() else {
                return;
            };

            let Some(tabs) = tabs.upgrade() else {
                return;
            };

            let stuck = tabs
                .borrow()
                .iter()
                .find(|tab| tab.id == id)
                .is_some_and(|tab| tab.nav_serial == serial && !tab.committed);

            if stuck && view.is_loading() {
                eprintln!("Load timed out: {target}");

                view.stop_loading();

                BrowserEngine::show_timeout_page(&view, &target);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loadable_targets_reach_a_tab() {
        assert!(is_tab_target("https://example.com/page"));
        assert!(is_tab_target("http://localhost:8080"));
        assert!(is_tab_target("file:///tmp/page.html"));
        assert!(is_tab_target("about:blank"));
        assert!(is_tab_target("axys://resources"));

        assert!(!is_tab_target("mailto:someone@example.com"));
        assert!(!is_tab_target("axys://does-not-exist"));
        assert!(!is_tab_target(""));
    }
}
