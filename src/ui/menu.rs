//! In-window popup replacements.
//!
//! The native popup surfaces (`GtkPopover`, `GtkMenuButton`, `GtkDropDown`)
//! proved unreliable on this project's main target (WSLg/RAIL links with
//! fractional scaling): freshly mapped popups could stay invisible,
//! misplaced or stacked under the window, with the button then refusing to
//! reopen them. Everything here is therefore plain in-window widgets:
//!
//! - [`MenuPanel`]: hamburger menu as an overlay panel;
//! - [`build_inline_picker`]: settings/welcome option lists as inline
//!   expanding rows (no floating surface at all).
//!
//! Behaviour contract (Chromium-like) for every panel:
//!
//! 1. button click with it closed → open;
//! 2. button click with it open → close;
//! 3. click on a panel item → apply + close;
//! 4. click anywhere else / Escape / resize → close.
//!
//! Item 4 for "anywhere else" is implemented by the window with a
//! full-size invisible catcher behind the panels (see `ui::window`).

use gtk::prelude::*;
use gtk::{Align, Box, Button, Image, Label, Orientation};
use std::rc::Rc;

/// (label i18n key, window action) rows of the hamburger menu, in order.
///
/// Page-scoped tools ("View page source", "Inspect Element") are deliberately
/// absent: they act on web content, not on the browser window, so they live
/// in the WebView's right-click menu instead.
fn menu_items(private_mode: bool) -> Vec<(&'static str, &'static str)> {
    let mut items = vec![
        ("app.new_tab", "win.new-tab"),
        ("app.new_window", "win.new-window"),
        ("app.reopen_tab", "win.reopen-tab"),
        ("history.title", "win.history"),
        ("app.downloads", "win.downloads"),
        ("tools.bookmarks", "win.bookmarks"),
        ("tools.resources", "win.resources"),
    ];

    // Toggle: inside a private window this item exits back to normal mode.
    if private_mode {
        items.push(("privacy.exit", "win.privacy"));
    } else {
        items.push(("app.privacy_window", "win.privacy"));
    }

    items.push(("app.tools", "win.tools"));
    items.push(("app.settings", "win.settings"));

    items
}

/// Hamburger menu panel. Shown as an overlay child by the window; hidden
/// by default. `on_close` just syncs window state (catcher); the panel
/// hides itself whenever a row is picked.
#[derive(Clone)]
pub struct MenuPanel {
    pub container: Box,
    rows: Box,
    on_close: Rc<dyn Fn()>,
    private_mode: bool,
}

impl MenuPanel {
    pub fn new(private_mode: bool, on_close: Rc<dyn Fn()>) -> Self {
        let container = Box::new(Orientation::Vertical, 2);

        container.add_css_class("menu-panel");

        container.set_visible(false);

        let rows = Box::new(Orientation::Vertical, 2);

        container.append(&rows);

        let panel = Self {
            container,
            rows,
            on_close,
            private_mode,
        };

        panel.rebuild_rows();

        panel
    }

    /// Shows, hides and toggles the panel.
    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn toggle(&self) {
        self.container.set_visible(!self.container.is_visible());
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    /// Rebuilds the rows (language change swaps every label).
    pub fn rebuild_rows(&self) {
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }

        for (label_key, action) in menu_items(self.private_mode) {
            let row = Button::new();

            row.add_css_class("menu-row");

            let label = Label::new(Some(&rust_i18n::t!(label_key)));

            label.set_halign(Align::Start);
            label.set_hexpand(true);

            row.set_child(Some(&label));

            // Weak: `self.container` is an ancestor of this row, so a strong
            // capture would be a `container -> rows -> row -> container`
            // cycle that leaks the whole panel.
            let container = self.container.downgrade();
            let on_close = self.on_close.clone();

            row.connect_clicked(move |row| {
                let _ = row.activate_action(action, None);

                if let Some(container) = container.upgrade() {
                    container.set_visible(false);
                }

                on_close();
            });

            self.rows.append(&row);
        }
    }
}

/// Inline option picker: a button that expands its option rows directly
/// below it (used in Settings and Welcome). Returns the toggle button and
/// the initially hidden rows box; the caller appends both in order.
pub fn build_inline_picker(
    options: Vec<String>,
    selected: usize,
    on_pick: Rc<dyn Fn(usize)>,
) -> (Button, Box) {
    let toggle = Button::new();

    let head = Box::new(Orientation::Horizontal, 6);

    let current = Label::new(options.get(selected).map(String::as_str));

    current.set_halign(Align::Start);
    current.set_hexpand(true);

    let arrow = Image::from_icon_name("pan-down-symbolic");

    head.append(&current);
    head.append(&arrow);

    toggle.set_child(Some(&head));

    let list = Box::new(Orientation::Vertical, 2);

    list.add_css_class("option-picker");

    // Shrink to the rows instead of spanning the whole page width.
    list.set_halign(Align::Start);

    list.set_visible(false);

    for (index, option) in options.iter().enumerate() {
        let row = Button::with_label(option);

        row.add_css_class("menu-row");

        // Weak: the row is a child of `list`, so capturing `list` (or the
        // sibling `toggle`/`current`) strongly here would make a GObject
        // reference cycle that leaks the whole picker.
        let row_list = list.downgrade();
        let toggle = toggle.downgrade();
        let current = current.downgrade();
        let text = option.clone();
        let on_pick = on_pick.clone();

        row.connect_clicked(move |_| {
            on_pick(index);

            if let Some(current) = current.upgrade() {
                current.set_text(&text);
            }

            if let Some(toggle) = toggle.upgrade() {
                toggle.set_tooltip_text(Some(&text));
            }

            if let Some(row_list) = row_list.upgrade() {
                row_list.set_visible(false);
            }
        });

        list.append(&row);
    }

    {
        let list = list.clone();

        toggle.connect_clicked(move |_| {
            list.set_visible(!list.is_visible());
        });
    }

    (toggle, list)
}

#[cfg(test)]
mod tests {
    use super::menu_items;

    fn has_action(items: &[(&'static str, &'static str)], action: &str) -> bool {
        items.iter().any(|(_, candidate)| *candidate == action)
    }

    #[test]
    fn resources_row_is_available_in_every_window() {
        assert!(has_action(&menu_items(false), "win.resources"));
        assert!(has_action(&menu_items(true), "win.resources"));
    }

    #[test]
    fn page_scoped_tools_stay_out_of_the_hamburger_menu() {
        for private_mode in [false, true] {
            let items = menu_items(private_mode);

            assert!(!has_action(&items, "win.view-source"));
            assert!(!has_action(&items, "win.devtools"));
        }
    }

    #[test]
    fn private_mode_swaps_the_privacy_row_label() {
        let normal = menu_items(false);
        let private = menu_items(true);

        assert!(
            normal
                .iter()
                .any(|(key, action)| *action == "win.privacy" && *key == "app.privacy_window")
        );
        assert!(
            private
                .iter()
                .any(|(key, action)| *action == "win.privacy" && *key == "privacy.exit")
        );
    }
}
