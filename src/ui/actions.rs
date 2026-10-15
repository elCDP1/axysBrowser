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
}

/// Registers the eight `win.*` actions on `window`.
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
        ("settings", "axys://settings"),
    ] {
        let new_tab = actions.new_tab.clone();

        let action = gio::SimpleAction::new(name, None);

        action.connect_activate(move |_, _| {
            new_tab(Some(uri));
        });

        window.add_action(&action);
    }
}
