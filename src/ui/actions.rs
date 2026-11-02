//! `win.*` window actions (hamburger menu targets, shortcuts backend).
//!
//! Extracted from `ui::window`: each action is a plain callback supplied by
//! the window, so this module never reaches back into window state.

use gtk::prelude::*;
use gtk::{ApplicationWindow, gio};
use std::rc::Rc;

use crate::browser::tabs::NewTabCallback;

/// Everything the actions need, as plain callbacks.
pub struct WindowActions {
    /// Open a tab; `None` = blank new tab, `Some(uri)` = starting page.
    pub new_tab: NewTabCallback,
    /// Reopen the most recently closed tab.
    pub reopen_closed_tab: Rc<dyn Fn()>,
    /// Open a fresh normal window.
    pub open_window: Rc<dyn Fn()>,
    /// Privacy toggle: open a private window, or (from a private window)
    /// go back to a normal one.
    pub open_private_toggle: Rc<dyn Fn()>,
    /// Show the active page's HTML source in a new tab.
    pub view_source: Rc<dyn Fn()>,
    /// Zoom the active page.
    pub zoom_in: Rc<dyn Fn()>,
    pub zoom_out: Rc<dyn Fn()>,
    pub zoom_reset: Rc<dyn Fn()>,
    /// Open the downloads directory in the system file manager.
    pub open_downloads_folder: Rc<dyn Fn()>,
    /// Print the active page (system print dialog).
    pub print: Rc<dyn Fn()>,
    /// Save the active page to a file (MHTML).
    pub save_page: Rc<dyn Fn()>,
    /// Open the find-in-page bar.
    pub find: Rc<dyn Fn()>,
    /// Toggle fullscreen for the window.
    pub fullscreen: Rc<dyn Fn()>,
    /// Bookmark (or edit the bookmark of) the active page.
    pub bookmark_page: Rc<dyn Fn()>,
    /// Focus the window's Settings tab, opening one only if it has none.
    pub focus_settings: Rc<dyn Fn()>,
}

/// Every `win.*` action registered below, without the `win.` prefix.
/// `register_window_actions` checks this list against what it really
/// registered (debug builds, which is what the smoke tests run), and a test
/// checks the hamburger menu against it, so a menu row can never point at an
/// action that does not exist.
pub const WINDOW_ACTION_NAMES: &[&str] = &[
    "new-tab",
    "new-window",
    "reopen-tab",
    "privacy",
    "tools",
    "downloads",
    "history",
    "bookmarks",
    "resources",
    "settings",
    "clear-data",
    "view-source",
    "zoom-in",
    "zoom-out",
    "zoom-reset",
    "open-downloads-folder",
    "print",
    "save-page",
    "find",
    "fullscreen",
    "bookmark-page",
];

/// Registers every `win.*` action on `window`.
pub fn register_window_actions(window: &ApplicationWindow, actions: WindowActions) {
    {
        let new_tab = actions.new_tab.clone();

        let action = gio::SimpleAction::new("new-tab", None);

        action.connect_activate(move |_, _| {
            new_tab(None);
        });

        window.add_action(&action);
    }

    {
        let open_window = actions.open_window.clone();

        let action = gio::SimpleAction::new("new-window", None);

        action.connect_activate(move |_, _| {
            open_window();
        });

        window.add_action(&action);
    }

    {
        let reopen_closed_tab = actions.reopen_closed_tab.clone();

        let action = gio::SimpleAction::new("reopen-tab", None);

        action.connect_activate(move |_, _| {
            reopen_closed_tab();
        });

        window.add_action(&action);
    }

    {
        let open_private_toggle = actions.open_private_toggle.clone();

        let action = gio::SimpleAction::new("privacy", None);

        action.connect_activate(move |_, _| {
            open_private_toggle();
        });

        window.add_action(&action);
    }

    for (name, uri) in [
        ("tools", "axys://tools"),
        ("downloads", "axys://downloads"),
        ("history", "axys://history"),
        ("bookmarks", "axys://bookmarks"),
        ("resources", "axys://resources"),
        ("settings", "axys://settings"),
    ] {
        let new_tab = actions.new_tab.clone();

        let action = gio::SimpleAction::new(name, None);

        action.connect_activate(move |_, _| {
            new_tab(Some(uri));
        });

        window.add_action(&action);
    }

    {
        // Chromium's "Clear browsing data": the Privacy section lives in the
        // settings page here. Reuse an open Settings tab instead of stacking
        // a new one on every click.
        let focus_settings = actions.focus_settings.clone();

        let action = gio::SimpleAction::new("clear-data", None);

        action.connect_activate(move |_, _| {
            focus_settings();
        });

        window.add_action(&action);
    }

    for (name, callback) in [
        ("view-source", actions.view_source.clone()),
        ("zoom-in", actions.zoom_in.clone()),
        ("zoom-out", actions.zoom_out.clone()),
        ("zoom-reset", actions.zoom_reset.clone()),
        (
            "open-downloads-folder",
            actions.open_downloads_folder.clone(),
        ),
        ("print", actions.print.clone()),
        ("save-page", actions.save_page.clone()),
        ("find", actions.find.clone()),
        ("fullscreen", actions.fullscreen.clone()),
        ("bookmark-page", actions.bookmark_page.clone()),
    ] {
        let action = gio::SimpleAction::new(name, None);

        action.connect_activate(move |_, _| {
            callback();
        });

        window.add_action(&action);
    }

    debug_assert!(
        WINDOW_ACTION_NAMES
            .iter()
            .all(|name| window.has_action(name)),
        "WINDOW_ACTION_NAMES lists an action that was not registered"
    );
}

#[cfg(test)]
mod tests {
    use super::WINDOW_ACTION_NAMES;
    use crate::ui::menu::MENU_ACTIONS;

    #[test]
    fn menu_actions_are_all_registered_window_actions() {
        for action in MENU_ACTIONS {
            let name = action.strip_prefix("win.").expect("menu actions are win.*");

            assert!(
                WINDOW_ACTION_NAMES.contains(&name),
                "{action} is in the menu but not registered"
            );
        }
    }

    #[test]
    fn registered_action_names_are_unique() {
        let mut names = WINDOW_ACTION_NAMES.to_vec();

        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), WINDOW_ACTION_NAMES.len());
    }
}
