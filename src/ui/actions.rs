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
    /// Open the WebKit inspector for the active page.
    pub devtools: Rc<dyn Fn()>,
    /// Zoom the active page.
    pub zoom_in: Rc<dyn Fn()>,
    pub zoom_out: Rc<dyn Fn()>,
    pub zoom_reset: Rc<dyn Fn()>,
    /// Open the downloads directory in the system file manager.
    pub open_downloads_folder: Rc<dyn Fn()>,
}

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
        ("tasks", "axys://tasks"),
        ("settings", "axys://settings"),
    ] {
        let new_tab = actions.new_tab.clone();

        let action = gio::SimpleAction::new(name, None);

        action.connect_activate(move |_, _| {
            new_tab(Some(uri));
        });

        window.add_action(&action);
    }

    for (name, callback) in [
        ("view-source", actions.view_source.clone()),
        ("devtools", actions.devtools.clone()),
        ("zoom-in", actions.zoom_in.clone()),
        ("zoom-out", actions.zoom_out.clone()),
        ("zoom-reset", actions.zoom_reset.clone()),
        (
            "open-downloads-folder",
            actions.open_downloads_folder.clone(),
        ),
    ] {
        let action = gio::SimpleAction::new(name, None);

        action.connect_activate(move |_, _| {
            callback();
        });

        window.add_action(&action);
    }
}
