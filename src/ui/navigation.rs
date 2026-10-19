//! Web-load helpers shared by the window's navigation paths.
//!
//! Extracted from `ui::window` so the main window file stays focused on
//! wiring: address-bar text rules and the hung-load watchdog live here.

use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::browser::engine::BrowserEngine;
use crate::browser::tabs::Tab;
use crate::internal::router;
use webkit6::WebView;
use webkit6::prelude::WebViewExt;

/// Seconds without committing a load before the watchdog gives up on it.
pub const LOAD_TIMEOUT_SECS: u64 = 60;

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
/// (different serial or URI) never kills a newer load.
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
    let tabs = tabs.clone();
    let target = target.to_string();

    gtk::glib::timeout_add_local_once(
        std::time::Duration::from_secs(LOAD_TIMEOUT_SECS),
        move || {
            let Some(view) = weak.upgrade() else {
                return;
            };

            let stuck = tabs
                .borrow()
                .iter()
                .find(|tab| tab.id == id)
                .is_some_and(|tab| tab.nav_serial == serial && !tab.committed && tab.uri == target);

            if stuck && view.is_loading() {
                eprintln!("Load timed out: {target}");

                view.stop_loading();

                BrowserEngine::show_timeout_page(&view, &target);
            }
        },
    );
}
