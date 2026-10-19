use gtk::Widget;
use gtk::glib::object::Cast;
use std::rc::Rc;

use crate::app_state::AppState;
use crate::browser::history::HistoryManager;

use super::pages::{
    about::build_about, bookmarks::build_bookmarks, downloads::build_downloads,
    history::build_history, newtab::build_newtab, privacy::build_privacy, settings::build_settings,
    tasks::TabRow, tasks::TaskDeps, tasks::build_tasks, tools::ToolAction, tools::ToolsDeps,
    tools::build_tools, welcome::build_welcome,
};

pub fn page_name(uri: &str) -> Option<&'static str> {
    match uri {
        "axys://" | "axys://newtab" => Some("newtab"),
        "axys://welcome" => Some("welcome"),
        "axys://privacy" => Some("privacy"),
        "axys://about" => Some("about"),
        "axys://settings" => Some("settings"),
        "axys://tools" => Some("tools"),
        "axys://downloads" => Some("downloads"),
        "axys://history" => Some("history"),
        "axys://bookmarks" => Some("bookmarks"),
        "axys://tasks" => Some("tasks"),
        _ => None,
    }
}

pub fn page_title(uri: &str) -> String {
    match uri {
        "axys://welcome" => rust_i18n::t!("welcome.title").to_string(),
        "axys://privacy" => rust_i18n::t!("privacy.title").to_string(),
        "axys://about" => rust_i18n::t!("about.title").to_string(),
        "axys://settings" => rust_i18n::t!("settings.title").to_string(),
        "axys://tools" => rust_i18n::t!("tools.title").to_string(),
        "axys://downloads" => rust_i18n::t!("downloads.title").to_string(),
        "axys://history" => rust_i18n::t!("history.title").to_string(),
        "axys://bookmarks" => rust_i18n::t!("tools.bookmarks").to_string(),
        "axys://tasks" => rust_i18n::t!("tools.tasks").to_string(),
        _ => rust_i18n::t!("app.new_tab").to_string(),
    }
}

/// Page callbacks shared by every internal page, kept together so
/// [`route`] stays readable as pages gain callbacks.
pub struct PageCallbacks {
    pub on_search: Rc<dyn Fn(String)>,
    pub on_about: Rc<dyn Fn()>,
    pub on_downloads: Rc<dyn Fn()>,
    pub on_history: Rc<dyn Fn()>,
    pub on_clear_browsing_data: crate::app_state::ClearBrowsingData,
    /// History store for this window (ephemeral in private windows).
    pub history: HistoryManager,
    /// Snapshot of the window's open tabs (task manager).
    pub tab_rows: Rc<dyn Fn() -> Vec<TabRow>>,
    pub on_close_tab: Rc<dyn Fn(usize)>,
    /// Runs a page/system action through the window's `win.*` actions.
    pub on_tool: Rc<dyn Fn(ToolAction)>,
    /// Zoom level of the active web view, if a page is loaded.
    pub zoom_level: Rc<dyn Fn() -> Option<f64>>,
}

pub fn route(
    uri: &str,
    callbacks: PageCallbacks,
    state: Rc<AppState>,
    downloads: crate::browser::downloads::DownloadManager,
) -> Option<Widget> {
    let PageCallbacks {
        on_search,
        on_about,
        on_downloads,
        on_history,
        on_clear_browsing_data,
        history,
        tab_rows,
        on_close_tab,
        on_tool,
        zoom_level,
    } = callbacks;

    match uri {
        "axys://" | "axys://newtab" => Some(build_newtab(on_search, on_about).upcast()),

        "axys://welcome" => Some(build_welcome(state, on_search).upcast()),

        "axys://privacy" => Some(build_privacy(on_search).upcast()),

        "axys://about" => Some(build_about().upcast()),

        "axys://settings" => Some(build_settings(state, on_clear_browsing_data).upcast()),

        "axys://tools" => Some(
            build_tools(ToolsDeps {
                on_downloads,
                on_history,
                on_clear_browsing_data,
                history: history.clone(),
                on_tool,
                zoom_level,
            })
            .upcast(),
        ),

        "axys://tasks" => Some(
            build_tasks(TaskDeps {
                tab_rows,
                on_close_tab,
            })
            .upcast(),
        ),

        "axys://downloads" => Some(build_downloads(downloads).upcast()),

        "axys://history" => Some(build_history(history, on_search).upcast()),

        "axys://bookmarks" => Some(build_bookmarks(state.bookmarks.clone(), on_search).upcast()),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGES: [(&str, &str); 10] = [
        ("axys://", "newtab"),
        ("axys://newtab", "newtab"),
        ("axys://welcome", "welcome"),
        ("axys://privacy", "privacy"),
        ("axys://about", "about"),
        ("axys://settings", "settings"),
        ("axys://tools", "tools"),
        ("axys://downloads", "downloads"),
        ("axys://history", "history"),
        ("axys://tasks", "tasks"),
    ];

    #[test]
    fn maps_every_known_internal_page() {
        for (uri, name) in PAGES {
            assert_eq!(page_name(uri), Some(name), "{uri}");
        }

        assert_eq!(page_name("axys://bookmarks"), Some("bookmarks"));
    }

    #[test]
    fn rejects_unknown_and_external_uris() {
        for uri in [
            "axys://nope",
            "axys://tasks/",
            "AXYS://tasks",
            "https://example.com",
            "file:///tmp/page.html",
            "",
        ] {
            assert_eq!(page_name(uri), None, "{uri}");
        }
    }

    #[test]
    fn every_internal_page_has_a_non_empty_title() {
        for (uri, _) in PAGES {
            let title = page_title(uri);
            assert!(!title.trim().is_empty(), "{uri}");
        }

        // Unknown internal pages fall back to the new-tab title.
        assert_eq!(page_title("axys://nope"), page_title("axys://"));
    }
}
