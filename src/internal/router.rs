use gtk::Widget;
use gtk::glib::object::Cast;
use std::rc::Rc;

use crate::app_state::AppState;

use super::pages::{
    about::build_about, downloads::build_downloads, history::build_history, newtab::build_newtab,
    privacy::build_privacy, settings::build_settings, tools::build_tools, welcome::build_welcome,
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
        _ => rust_i18n::t!("app.new_tab").to_string(),
    }
}

#[expect(clippy::too_many_arguments)]
pub fn route(
    uri: &str,
    on_search: Rc<dyn Fn(String)>,
    on_about: Rc<dyn Fn()>,
    on_extensions_changed: Rc<dyn Fn(bool)>,
    on_downloads: Rc<dyn Fn()>,
    on_history: Rc<dyn Fn()>,
    on_clear_browsing_data: Rc<dyn Fn()>,
    state: Rc<AppState>,
) -> Option<Widget> {
    match uri {
        "axys://" | "axys://newtab" => Some(build_newtab(on_search, on_about).upcast()),

        "axys://welcome" => Some(build_welcome().upcast()),

        "axys://privacy" => Some(build_privacy().upcast()),

        "axys://about" => Some(build_about().upcast()),

        "axys://settings" => Some(build_settings(state, on_extensions_changed).upcast()),

        "axys://tools" => {
            Some(build_tools(on_downloads, on_history, on_clear_browsing_data).upcast())
        }

        "axys://downloads" => Some(build_downloads(state).upcast()),

        "axys://history" => Some(build_history(state.history.clone(), on_search).upcast()),

        _ => None,
    }
}
