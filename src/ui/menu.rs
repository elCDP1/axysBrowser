//! In-window popup replacements.
//!
//! The native popup surfaces (`GtkPopover`, `GtkMenuButton`, `GtkDropDown`)
//! proved unreliable on this project's main target (WSLg/RAIL links with
//! fractional scaling): freshly mapped popups could stay invisible,
//! misplaced or stacked under the window, with the button then refusing to
//! reopen them. Everything here is therefore plain in-window widgets:
//!
//! - [`MenuPanel`]: hamburger menu as an overlay panel;
//! - [`PickerPanel`]: Settings/Welcome option lists, floating over the page
//!   instead of pushing its content down;
//! - [`build_picker`]: the toggle button that asks the window to show it.
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
use gtk::{Align, Box, Button, Image, Label, Orientation, ScrolledWindow, Separator};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The hamburger list grows with the window, up to this height, and scrolls
/// beyond it, so a short window never clips the last rows (Settings is the
/// last one). The margin leaves room for the toolbar above the panel.
const MENU_MAX_HEIGHT: i32 = 700;
const MENU_MIN_HEIGHT: i32 = 200;
const MENU_VERTICAL_MARGIN: i32 = 190;

/// Width reserved for the tab menu when clamping it to the window: the rows
/// request 260 px and the panel adds padding and a border.
const TAB_MENU_WIDTH: i32 = 280;

/// Same idea for the option picker (rows request 180 px plus padding).
const PICKER_WIDTH: i32 = 200;

/// Rows that act on web content. They are disabled while an internal
/// `axys://` page is showing, where they would do nothing.
const PAGE_SCOPED_ACTIONS: &[&str] = &["win.print", "win.save-page", "win.bookmark-page"];

/// Height the list may use inside a window that is `window_height` tall.
fn menu_max_height(window_height: i32) -> i32 {
    (window_height - MENU_VERTICAL_MARGIN).clamp(MENU_MIN_HEIGHT, MENU_MAX_HEIGHT)
}

/// Window actions the hamburger menu may point at. Every name must be
/// registered by `ui::actions` (a test checks the menu against this list).
#[cfg(test)]
pub const MENU_ACTIONS: &[&str] = &[
    "win.new-tab",
    "win.new-window",
    "win.privacy",
    "win.reopen-tab",
    "win.history",
    "win.downloads",
    "win.bookmarks",
    "win.bookmark-page",
    "win.zoom-in",
    "win.zoom-out",
    "win.zoom-reset",
    "win.fullscreen",
    "win.print",
    "win.find",
    "win.save-page",
    "win.clear-data",
    "win.resources",
    "win.tools",
    "win.settings",
];

/// One line of the hamburger menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    /// (label i18n key, window action, shortcut hint shown on the right)
    Item(&'static str, &'static str, &'static str),
    Separator,
    /// "Zoom  −  100%  +  ⛶": keeps the menu open on − / + like Chromium.
    Zoom,
}

/// The menu, in Chromium's grouping: windows and tabs, browsing data,
/// zoom, page actions, browser tools, settings.
///
/// Page-scoped developer tools ("View page source", "Inspect Element") are
/// deliberately absent: they act on web content, not on the browser window,
/// so they live in the WebView's right-click menu instead.
fn menu_entries(private_mode: bool) -> Vec<Entry> {
    use Entry::{Item, Separator, Zoom};

    // Toggle: inside a private window this item exits back to normal mode.
    let privacy = if private_mode {
        Item("privacy.exit", "win.privacy", "Ctrl+Shift+N")
    } else {
        Item("app.privacy_window", "win.privacy", "Ctrl+Shift+N")
    };

    vec![
        Item("app.new_tab", "win.new-tab", "Ctrl+T"),
        Item("app.new_window", "win.new-window", "Ctrl+N"),
        privacy,
        Item("app.reopen_tab", "win.reopen-tab", "Ctrl+Shift+T"),
        Separator,
        Item("history.title", "win.history", "Ctrl+H"),
        Item("app.downloads", "win.downloads", "Ctrl+J"),
        Item("tools.bookmarks", "win.bookmarks", "Ctrl+Shift+O"),
        Item("menu.bookmark_page", "win.bookmark-page", "Ctrl+D"),
        Separator,
        Zoom,
        Separator,
        Item("menu.print", "win.print", "Ctrl+P"),
        Item("menu.find", "win.find", "Ctrl+F"),
        Item("menu.save_page", "win.save-page", "Ctrl+S"),
        Separator,
        Item("menu.clear_data", "win.clear-data", "Ctrl+Shift+Del"),
        Item("tools.resources", "win.resources", ""),
        Item("app.tools", "win.tools", ""),
        Separator,
        Item("app.settings", "win.settings", ""),
    ]
}

/// Splits the entries into the scrolling list and the pinned last group
/// (everything after the final separator). The separator itself opens the
/// pinned group so it stays attached to it.
fn split_footer(entries: &[Entry]) -> (&[Entry], &[Entry]) {
    match entries.iter().rposition(|entry| *entry == Entry::Separator) {
        Some(index) => entries.split_at(index),
        None => (entries, &[]),
    }
}

/// "100%" for the zoom row; `None` (no zoomable page) reads as 100%.
fn format_zoom(level: Option<f64>) -> String {
    format!("{}%", (level.unwrap_or(1.0) * 100.0).round() as i64)
}

/// Hamburger menu panel. Shown as an overlay child by the window; hidden
/// by default. `on_close` just syncs window state (catcher); the panel
/// hides itself whenever a row is picked.
#[derive(Clone)]
pub struct MenuPanel {
    pub container: Box,
    rows: Box,
    /// Last group (Settings): pinned below the scrolling list so it is
    /// always visible, however short the window.
    footer: Box,
    scroller: ScrolledWindow,
    zoom_label: Label,
    zoom_level: Rc<dyn Fn() -> Option<f64>>,
    /// Rows that only make sense on web content (see `PAGE_SCOPED_ACTIONS`).
    page_rows: RefCell<Vec<Button>>,
    page_available: Rc<dyn Fn() -> bool>,
    on_close: Rc<dyn Fn()>,
    private_mode: bool,
}

impl MenuPanel {
    pub fn new(
        private_mode: bool,
        on_close: Rc<dyn Fn()>,
        zoom_level: Rc<dyn Fn() -> Option<f64>>,
        page_available: Rc<dyn Fn() -> bool>,
    ) -> Self {
        let container = Box::new(Orientation::Vertical, 0);

        container.add_css_class("menu-panel");

        container.set_visible(false);

        let rows = Box::new(Orientation::Vertical, 2);

        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true)
            .propagate_natural_width(true)
            .max_content_height(menu_max_height(720))
            .min_content_width(300)
            .child(&rows)
            .build();

        container.append(&scroller);

        let footer = Box::new(Orientation::Vertical, 2);

        container.append(&footer);

        let panel = Self {
            container,
            rows,
            footer,
            scroller,
            zoom_label: Label::new(Some(&format_zoom(None))),
            zoom_level,
            page_rows: RefCell::new(Vec::new()),
            page_available,
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
        let show = !self.container.is_visible();

        if show {
            self.refresh_zoom();
            self.refresh_page_actions();

            // Fit the current window: the list scrolls only when it must.
            if let Some(root) = self.container.root() {
                let window_height = root.upcast_ref::<gtk::Widget>().height();

                self.scroller
                    .set_max_content_height(menu_max_height(window_height));
            }
        }

        self.container.set_visible(show);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    fn refresh_zoom(&self) {
        self.zoom_label.set_text(&format_zoom((self.zoom_level)()));
    }

    /// Enables or disables the rows that only work on web content.
    fn refresh_page_actions(&self) {
        let enabled = (self.page_available)();

        for row in self.page_rows.borrow().iter() {
            row.set_sensitive(enabled);
        }
    }

    /// Rebuilds the rows (language change swaps every label).
    pub fn rebuild_rows(&self) {
        for list in [&self.rows, &self.footer] {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
        }

        self.page_rows.borrow_mut().clear();

        let entries = menu_entries(self.private_mode);
        let (scrolling, pinned) = split_footer(&entries);

        for (list, group) in [(&self.rows, scrolling), (&self.footer, pinned)] {
            for entry in group {
                match entry {
                    Entry::Separator => {
                        let separator = Separator::new(Orientation::Horizontal);

                        separator.add_css_class("menu-separator");

                        list.append(&separator);
                    }
                    Entry::Zoom => list.append(&self.build_zoom_row()),
                    Entry::Item(label_key, action, shortcut) => {
                        list.append(&self.build_item_row(label_key, action, shortcut));
                    }
                }
            }
        }

        self.refresh_page_actions();
    }

    fn build_item_row(&self, label_key: &str, action: &'static str, shortcut: &str) -> Button {
        let row = Button::new();

        row.add_css_class("menu-row");

        if PAGE_SCOPED_ACTIONS.contains(&action) {
            self.page_rows.borrow_mut().push(row.clone());
        }

        let line = Box::new(Orientation::Horizontal, 24);

        let label = Label::new(Some(&rust_i18n::t!(label_key)));

        label.set_halign(Align::Start);
        label.set_hexpand(true);

        line.append(&label);

        if !shortcut.is_empty() {
            let hint = Label::new(Some(shortcut));

            hint.add_css_class("menu-shortcut");
            hint.set_halign(Align::End);

            line.append(&hint);
        }

        row.set_child(Some(&line));

        // Weak: `self.container` is an ancestor of this row, so a strong
        // capture would be a `container -> rows -> row -> container`
        // cycle that leaks the whole panel.
        let container = self.container.downgrade();
        let on_close = self.on_close.clone();

        row.connect_clicked(move |row| {
            // Close first: some actions (print) run a nested main loop and
            // would otherwise leave the menu open behind their dialog.
            if let Some(container) = container.upgrade() {
                container.set_visible(false);
            }

            on_close();

            let _ = row.activate_action(action, None);
        });

        row
    }

    /// "Zoom  −  100%  +  ⛶". The − / + / % buttons act on the page and keep
    /// the menu open so the zoom can be adjusted repeatedly; only the
    /// fullscreen button closes it.
    fn build_zoom_row(&self) -> Box {
        let line = Box::new(Orientation::Horizontal, 4);

        line.add_css_class("menu-zoom-row");

        let title = Label::new(Some(&rust_i18n::t!("menu.zoom")));

        title.set_halign(Align::Start);
        title.set_hexpand(true);

        line.append(&title);

        // Weak for the same ancestor-cycle reason as the item rows: the
        // label and the buttons live inside the panel.
        let step = |text: &str, action: &'static str| {
            let button = Button::with_label(text);

            button.add_css_class("menu-zoom-button");

            let label = self.zoom_label.downgrade();
            let zoom_level = self.zoom_level.clone();

            button.connect_clicked(move |button| {
                let _ = button.activate_action(action, None);

                if let Some(label) = label.upgrade() {
                    label.set_text(&format_zoom(zoom_level()));
                }
            });

            button
        };

        self.zoom_label.add_css_class("menu-zoom-value");
        self.zoom_label.set_width_chars(5);

        let reset = step("", "win.zoom-reset");

        reset.set_child(Some(&self.zoom_label));
        reset.set_tooltip_text(Some(&rust_i18n::t!("menu.zoom_reset")));

        line.append(&step("−", "win.zoom-out"));
        line.append(&reset);
        line.append(&step("+", "win.zoom-in"));

        // A text glyph, not an icon: icon names depend on the user's icon
        // theme and render as a generic placeholder where one is missing.
        let fullscreen = Button::with_label("⛶");

        fullscreen.add_css_class("menu-zoom-button");
        fullscreen.set_tooltip_text(Some(&rust_i18n::t!("menu.fullscreen")));

        let container = self.container.downgrade();
        let on_close = self.on_close.clone();

        fullscreen.connect_clicked(move |button| {
            if let Some(container) = container.upgrade() {
                container.set_visible(false);
            }

            on_close();

            let _ = button.activate_action("win.fullscreen", None);
        });

        line.append(&fullscreen);

        line
    }
}

/// What a right-click on a tab can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Reload,
    Duplicate,
    ToggleMute,
    Close,
    CloseOthers,
    CloseToTheRight,
}

/// One line of the tab context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabEntry {
    Item {
        action: TabAction,
        label_key: &'static str,
        shortcut: &'static str,
        enabled: bool,
    },
    Separator,
}

/// What the tab menu needs to know about the tab it was opened on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabMenuState {
    /// The tab's own web view is muted.
    pub muted: bool,
    /// A real web page (http, https, file): internal `axys://` pages have no
    /// audio to mute.
    pub is_web_page: bool,
    /// Other tabs in this window.
    pub others: usize,
    /// Tabs after this one.
    pub to_the_right: usize,
}

/// Chromium's tab menu, minus the entries this browser has no feature for
/// (pinning, groups, moving to another window).
pub fn tab_menu_entries(state: &TabMenuState) -> Vec<TabEntry> {
    let item = |action, label_key, shortcut, enabled| TabEntry::Item {
        action,
        label_key,
        shortcut,
        enabled,
    };

    let mut entries = vec![
        item(TabAction::Reload, "tabs.reload", "Ctrl+R", true),
        item(TabAction::Duplicate, "tabs.duplicate", "", true),
    ];

    if state.is_web_page {
        let label = if state.muted {
            "tabs.unmute"
        } else {
            "tabs.mute"
        };

        entries.push(item(TabAction::ToggleMute, label, "", true));
    }

    entries.extend([
        TabEntry::Separator,
        item(TabAction::Close, "tabs.close", "Ctrl+W", true),
        item(
            TabAction::CloseOthers,
            "tabs.close_others",
            "",
            state.others > 0,
        ),
        item(
            TabAction::CloseToTheRight,
            "tabs.close_right",
            "",
            state.to_the_right > 0,
        ),
    ]);

    entries
}

/// Tab context menu, as an overlay panel like [`MenuPanel`] (no popup
/// surface). Hidden by default; the window places it under the clicked tab.
#[derive(Clone)]
pub struct TabMenuPanel {
    pub container: Box,
    rows: Box,
    on_close: Rc<dyn Fn()>,
}

impl TabMenuPanel {
    pub fn new(on_close: Rc<dyn Fn()>) -> Self {
        let container = Box::new(Orientation::Vertical, 0);

        container.add_css_class("menu-panel");
        container.set_visible(false);

        let rows = Box::new(Orientation::Vertical, 2);

        rows.set_width_request(260);

        container.append(&rows);

        Self {
            container,
            rows,
            on_close,
        }
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    /// Shows the menu `left` pixels from the window's left edge. `run`
    /// receives the picked action after the menu has closed.
    pub fn show(&self, left: i32, entries: &[TabEntry], run: Rc<dyn Fn(TabAction)>) {
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }

        for entry in entries {
            match *entry {
                TabEntry::Separator => {
                    let separator = Separator::new(Orientation::Horizontal);

                    separator.add_css_class("menu-separator");

                    self.rows.append(&separator);
                }
                TabEntry::Item {
                    action,
                    label_key,
                    shortcut,
                    enabled,
                } => {
                    let row = Button::new();

                    row.add_css_class("menu-row");
                    row.set_sensitive(enabled);

                    let line = Box::new(Orientation::Horizontal, 24);
                    let label = Label::new(Some(&rust_i18n::t!(label_key)));

                    label.set_halign(Align::Start);
                    label.set_hexpand(true);

                    line.append(&label);

                    if !shortcut.is_empty() {
                        let hint = Label::new(Some(shortcut));

                        hint.add_css_class("menu-shortcut");
                        hint.set_halign(Align::End);

                        line.append(&hint);
                    }

                    row.set_child(Some(&line));

                    // Weak: the container is this row's ancestor.
                    let container = self.container.downgrade();
                    let on_close = self.on_close.clone();
                    let run = run.clone();

                    row.connect_clicked(move |_| {
                        if let Some(container) = container.upgrade() {
                            container.set_visible(false);
                        }

                        on_close();

                        run(action);
                    });

                    self.rows.append(&row);
                }
            }
        }

        // Keep the panel inside the window when the tab sits near the right
        // edge: the rows request 260 px plus the panel padding and border.
        let window_width = self
            .container
            .root()
            .map(|root| root.width())
            .unwrap_or(i32::MAX);

        self.container
            .set_margin_start(left.max(0).min((window_width - TAB_MENU_WIDTH).max(0)));
        self.container.set_visible(true);

        (self.on_close)();
    }
}

/// Opens the floating option picker for a toggle button.
///
/// Internal pages do not own the window overlay, so they receive this
/// callback (through `PageCallbacks`) instead of embedding the rows.
pub type ShowPicker = Rc<dyn Fn(Button, Vec<String>, usize, Rc<dyn Fn(usize)>)>;

/// Option toggle (Settings/Welcome): shows the current value and asks the
/// window to open its floating picker under this button, so the page layout
/// never moves.
pub fn build_picker(
    options: Vec<String>,
    selected: usize,
    on_pick: Rc<dyn Fn(usize)>,
    show_picker: ShowPicker,
) -> Button {
    let toggle = Button::new();

    let head = Box::new(Orientation::Horizontal, 6);

    let current = Label::new(options.get(selected).map(String::as_str));

    current.set_halign(Align::Start);
    current.set_hexpand(true);

    let arrow = Image::from_icon_name("pan-down-symbolic");

    head.append(&current);
    head.append(&arrow);

    toggle.set_child(Some(&head));

    if let Some(text) = options.get(selected) {
        toggle.set_tooltip_text(Some(text));
    }

    let picked = Rc::new(Cell::new(selected));

    // Runs the caller's setter and keeps the button label in sync. Weak to
    // the label and button: they are this button's own descendants.
    let choose: Rc<dyn Fn(usize)> = {
        let current = current.downgrade();
        let toggle = toggle.downgrade();
        let options = options.clone();
        let picked = picked.clone();
        let on_pick = on_pick.clone();

        Rc::new(move |index| {
            on_pick(index);
            picked.set(index);

            let Some(text) = options.get(index) else {
                return;
            };

            if let Some(current) = current.upgrade() {
                current.set_text(text);
            }

            if let Some(toggle) = toggle.upgrade() {
                toggle.set_tooltip_text(Some(text));
            }
        })
    };

    {
        let options = options.clone();
        let picked = picked.clone();

        toggle.connect_clicked(move |toggle| {
            show_picker(
                toggle.clone(),
                options.clone(),
                picked.get(),
                choose.clone(),
            );
        });
    }

    toggle
}

/// Where an option list goes inside the content area.
///
/// Pure so the edge cases (flip above, tiny window, right-side control) can
/// be tested without a display. Prefers below the toggle, falls back to
/// above, and finally pins the panel inside the area so it can never be
/// placed off-screen.
#[allow(clippy::too_many_arguments)]
fn picker_position(
    anchor_left: i32,
    anchor_top: i32,
    anchor_width: i32,
    anchor_height: i32,
    panel_width: i32,
    panel_height: i32,
    area_width: i32,
    area_height: i32,
) -> (i32, i32) {
    let below = anchor_top + anchor_height + 4;
    let above = anchor_top - panel_height - 4;

    let top = if below + panel_height <= area_height {
        below
    } else if above >= 0 {
        above
    } else {
        (area_height - panel_height - 8).max(0)
    };

    // Right-side controls (the cookies row puts its toggle at the end) align
    // their menu's right edge with the toggle, so it opens under the control
    // instead of jumping left to fit. Everything else keeps the left edge.
    let anchor_left = anchor_left.max(0);
    let anchor_right = anchor_left + anchor_width;
    let max_left = (area_width - panel_width).max(0);

    let left = if anchor_left + anchor_width / 2 > area_width / 2 {
        anchor_right - panel_width
    } else {
        anchor_left
    };

    (left.max(0).min(max_left), top.max(0))
}

/// Closes the panels when `area` changes size, per the module contract
/// (item 4: resize closes). `area` is the overlay that holds the panels.
pub fn close_panels_on_resize<W>(area: &W, on_close: Rc<dyn Fn()>)
where
    W: IsA<gtk::Widget> + 'static,
{
    let width_close = on_close.clone();

    area.connect_notify_local(Some("width"), move |_, _| width_close());
    area.connect_notify_local(Some("height"), move |_, _| on_close());
}

/// Floating option list, parented by the window overlay like [`MenuPanel`],
/// so opening it never reflows the page.
#[derive(Clone)]
pub struct PickerPanel {
    pub container: Box,
    scroller: ScrolledWindow,
    rows: Box,
    /// Toggle this picker is open for, so clicking it again closes the list.
    anchor: RefCell<Option<gtk::glib::WeakRef<Button>>>,
    on_close: Rc<dyn Fn()>,
}

impl PickerPanel {
    pub fn new(on_close: Rc<dyn Fn()>) -> Self {
        let container = Box::new(Orientation::Vertical, 0);

        container.add_css_class("menu-panel");
        container.set_visible(false);

        let rows = Box::new(Orientation::Vertical, 2);

        rows.set_width_request(180);

        // Long option lists must scroll inside the panel instead of growing
        // past the content area. The maximum height is refreshed on show.
        let scroller = ScrolledWindow::new();

        scroller.add_css_class("picker-scroller");
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_propagate_natural_width(true);
        scroller.set_propagate_natural_height(true);
        scroller.set_has_frame(false);
        scroller.set_child(Some(&rows));

        container.append(&scroller);

        Self {
            container,
            scroller,
            rows,
            anchor: RefCell::new(None),
            on_close,
        }
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        *self.anchor.borrow_mut() = None;
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    /// Shows the options under `anchor`. Clicking the same toggle again
    /// closes the list.
    ///
    /// The position is computed against the overlay that owns this panel, so
    /// it stays correct inside scrolled pages and when the window resizes.
    pub fn show(
        &self,
        anchor: &Button,
        options: &[String],
        selected: usize,
        on_pick: Rc<dyn Fn(usize)>,
    ) {
        let same_anchor = self
            .anchor
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
            .is_some_and(|current| &current == anchor);

        if self.container.is_visible() && same_anchor {
            self.hide();
            (self.on_close)();

            return;
        }

        // The window parents this panel to its content overlay; without it
        // there is nowhere to place the list (page built before the toolbar).
        let Some(overlay) = self.container.parent() else {
            return;
        };

        let Some(point) = anchor.compute_point(&overlay, &gtk::graphene::Point::new(0.0, 0.0))
        else {
            return;
        };

        let anchor_left = point.x() as i32;
        let anchor_top = point.y() as i32;

        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }

        for (index, option) in options.iter().enumerate() {
            let row = Button::with_label(option);

            row.add_css_class("menu-row");

            if index == selected {
                row.add_css_class("menu-row-selected");
            }

            let container = self.container.downgrade();
            let on_close = self.on_close.clone();
            let on_pick = on_pick.clone();

            row.connect_clicked(move |_| {
                if let Some(container) = container.upgrade() {
                    container.set_visible(false);
                }

                on_close();

                on_pick(index);
            });

            self.rows.append(&row);
        }

        *self.anchor.borrow_mut() = Some(anchor.downgrade());

        // Never let the list grow past the content area: long option lists
        // scroll inside the panel instead of being clipped by the window.
        // The 20 px reserve covers the panel's padding and border.
        self.scroller
            .set_max_content_height((overlay.height() - 20).max(40));

        // Show at the origin first so the real (not estimated) size can be
        // measured; the final placement is applied before the next paint.
        self.container.set_margin_start(0);
        self.container.set_margin_top(0);
        self.container.set_visible(true);

        let area_width = overlay.width();
        let area_height = overlay.height();

        let panel_width = self
            .container
            .measure(gtk::Orientation::Horizontal, -1)
            .1
            .max(PICKER_WIDTH);

        let panel_height = self
            .container
            .measure(gtk::Orientation::Vertical, panel_width)
            .1;

        let (left, top) = picker_position(
            anchor_left,
            anchor_top,
            anchor.width(),
            anchor.height(),
            panel_width,
            panel_height,
            area_width,
            area_height,
        );

        self.container.set_margin_start(left);
        self.container.set_margin_top(top);

        (self.on_close)();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Entry, MENU_ACTIONS, MENU_MAX_HEIGHT, MENU_MIN_HEIGHT, TabAction, TabEntry, TabMenuState,
        format_zoom, menu_entries, menu_max_height, picker_position, split_footer,
        tab_menu_entries,
    };

    /// Convenience wrapper: a 100x30 toggle at (x, y) in an 800x600 area.
    fn place(anchor_left: i32, anchor_top: i32, panel_width: i32, panel_height: i32) -> (i32, i32) {
        picker_position(
            anchor_left,
            anchor_top,
            100,
            30,
            panel_width,
            panel_height,
            800,
            600,
        )
    }

    #[test]
    fn picker_opens_below_when_it_fits() {
        assert_eq!(place(100, 100, 200, 120), (100, 134));
    }

    #[test]
    fn picker_flips_above_when_the_bottom_is_too_close() {
        // 534 + 120 > 600, and the real panel height leaves room above.
        assert_eq!(place(100, 500, 200, 120), (100, 376));
    }

    #[test]
    fn picker_pins_inside_when_neither_side_fits() {
        let (_, top) = picker_position(100, 80, 100, 30, 200, 120, 800, 200);

        assert_eq!(top, 72);
    }

    #[test]
    fn right_side_controls_align_their_right_edges() {
        let (left, _) = picker_position(700, 100, 80, 30, 200, 120, 800, 600);

        // Panel spans 580..780: the right edge matches the toggle's.
        assert_eq!(left, 580);
    }

    #[test]
    fn left_side_controls_keep_their_left_edge() {
        let (left, _) = place(40, 100, 200, 120);

        assert_eq!(left, 40);
    }

    #[test]
    fn picker_never_spills_past_the_right_edge() {
        let (left, _) = picker_position(780, 100, 100, 30, 200, 120, 800, 600);

        assert_eq!(left, 600);
    }

    #[test]
    fn picker_never_goes_off_the_left_edge() {
        let (left, _) = picker_position(0, 100, 100, 30, 900, 120, 800, 600);

        assert_eq!(left, 0);
    }

    fn items(private_mode: bool) -> Vec<(&'static str, &'static str, &'static str)> {
        menu_entries(private_mode)
            .into_iter()
            .filter_map(|entry| match entry {
                Entry::Item(label, action, shortcut) => Some((label, action, shortcut)),
                _ => None,
            })
            .collect()
    }

    fn has_action(private_mode: bool, action: &str) -> bool {
        items(private_mode)
            .iter()
            .any(|(_, candidate, _)| *candidate == action)
    }

    #[test]
    fn resources_row_is_available_in_every_window() {
        assert!(has_action(false, "win.resources"));
        assert!(has_action(true, "win.resources"));
    }

    #[test]
    fn page_scoped_tools_stay_out_of_the_hamburger_menu() {
        for private_mode in [false, true] {
            assert!(!has_action(private_mode, "win.view-source"));
            assert!(!has_action(private_mode, "win.devtools"));
        }
    }

    #[test]
    fn private_mode_swaps_the_privacy_row_label() {
        assert!(
            items(false)
                .iter()
                .any(|(key, action, _)| *action == "win.privacy" && *key == "app.privacy_window")
        );
        assert!(
            items(true)
                .iter()
                .any(|(key, action, _)| *action == "win.privacy" && *key == "privacy.exit")
        );
    }

    #[test]
    fn every_menu_action_is_a_known_window_action() {
        for private_mode in [false, true] {
            for (_, action, _) in items(private_mode) {
                assert!(
                    MENU_ACTIONS.contains(&action),
                    "{action} is not in MENU_ACTIONS"
                );
            }
        }
    }

    #[test]
    fn chromium_page_actions_are_reachable_from_the_menu() {
        for action in [
            "win.print",
            "win.find",
            "win.save-page",
            "win.clear-data",
            "win.bookmark-page",
            "win.bookmarks",
        ] {
            assert!(has_action(false, action), "{action} missing");
        }
    }

    #[test]
    fn separators_only_sit_between_groups_and_zoom_appears_once() {
        for private_mode in [false, true] {
            let entries = menu_entries(private_mode);

            assert_ne!(entries.first(), Some(&Entry::Separator));
            assert_ne!(entries.last(), Some(&Entry::Separator));
            assert!(
                entries
                    .windows(2)
                    .all(|pair| pair != [Entry::Separator, Entry::Separator]),
                "two separators in a row"
            );
            assert_eq!(entries.iter().filter(|e| **e == Entry::Zoom).count(), 1);
        }
    }

    #[test]
    fn shortcut_hints_are_unique() {
        let hints: Vec<_> = items(false)
            .into_iter()
            .map(|(_, _, shortcut)| shortcut)
            .filter(|shortcut| !shortcut.is_empty())
            .collect();

        let mut unique = hints.clone();

        unique.sort_unstable();
        unique.dedup();

        assert_eq!(hints.len(), unique.len());
    }

    #[test]
    fn zoom_is_shown_as_a_rounded_percentage() {
        assert_eq!(format_zoom(None), "100%");
        assert_eq!(format_zoom(Some(1.0)), "100%");
        assert_eq!(format_zoom(Some(1.1)), "110%");
        assert_eq!(format_zoom(Some(0.9999999)), "100%");
        assert_eq!(format_zoom(Some(0.25)), "25%");
        assert_eq!(format_zoom(Some(5.0)), "500%");
    }

    #[test]
    fn menu_height_follows_the_window_within_bounds() {
        assert_eq!(menu_max_height(720), 530);
        assert_eq!(menu_max_height(2000), MENU_MAX_HEIGHT);
        assert_eq!(menu_max_height(100), MENU_MIN_HEIGHT);
        assert_eq!(menu_max_height(0), MENU_MIN_HEIGHT);
    }

    #[test]
    fn settings_is_pinned_in_the_footer_in_every_window() {
        for private_mode in [false, true] {
            let entries = menu_entries(private_mode);
            let (scrolling, pinned) = split_footer(&entries);

            assert_eq!(
                pinned,
                [
                    Entry::Separator,
                    Entry::Item("app.settings", "win.settings", "")
                ]
            );
            assert!(!scrolling.contains(&Entry::Item("app.settings", "win.settings", "")));
            assert_eq!(scrolling.len() + pinned.len(), entries.len());
        }
    }

    #[test]
    fn split_footer_handles_menus_without_separators() {
        let only = [Entry::Zoom];

        assert_eq!(split_footer(&only), (&only[..], &[][..]));
        assert_eq!(split_footer(&[]), (&[][..], &[][..]));
    }

    fn tab_state(muted: bool, web: bool, others: usize, right: usize) -> TabMenuState {
        TabMenuState {
            muted,
            is_web_page: web,
            others,
            to_the_right: right,
        }
    }

    fn tab_items(state: &TabMenuState) -> Vec<(TabAction, &'static str, bool)> {
        tab_menu_entries(state)
            .into_iter()
            .filter_map(|entry| match entry {
                TabEntry::Item {
                    action,
                    label_key,
                    enabled,
                    ..
                } => Some((action, label_key, enabled)),
                TabEntry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn tab_menu_mute_entry_only_exists_for_web_pages_and_flips_its_label() {
        let page = tab_items(&tab_state(false, true, 1, 1));
        let muted = tab_items(&tab_state(true, true, 1, 1));
        let internal = tab_items(&tab_state(false, false, 1, 1));

        assert!(page.contains(&(TabAction::ToggleMute, "tabs.mute", true)));
        assert!(muted.contains(&(TabAction::ToggleMute, "tabs.unmute", true)));
        assert!(
            internal
                .iter()
                .all(|(action, _, _)| *action != TabAction::ToggleMute)
        );
    }

    #[test]
    fn tab_menu_disables_close_others_and_close_right_when_there_is_nothing_to_close() {
        let alone = tab_items(&tab_state(false, true, 0, 0));

        assert!(alone.contains(&(TabAction::CloseOthers, "tabs.close_others", false)));
        assert!(alone.contains(&(TabAction::CloseToTheRight, "tabs.close_right", false)));
        assert!(alone.contains(&(TabAction::Close, "tabs.close", true)));

        let last_of_three = tab_items(&tab_state(false, true, 2, 0));

        assert!(last_of_three.contains(&(TabAction::CloseOthers, "tabs.close_others", true)));
        assert!(last_of_three.contains(&(TabAction::CloseToTheRight, "tabs.close_right", false)));
    }

    #[test]
    fn tab_menu_always_offers_reload_duplicate_and_close() {
        for state in [tab_state(false, true, 0, 0), tab_state(true, false, 3, 2)] {
            let actions: Vec<_> = tab_items(&state)
                .into_iter()
                .map(|(action, _, _)| action)
                .collect();

            for needed in [TabAction::Reload, TabAction::Duplicate, TabAction::Close] {
                assert!(actions.contains(&needed), "{needed:?} missing");
            }
        }
    }

    #[test]
    fn tab_menu_has_one_separator_between_its_groups() {
        let entries = tab_menu_entries(&tab_state(false, true, 1, 1));

        assert_eq!(
            entries
                .iter()
                .filter(|e| **e == TabEntry::Separator)
                .count(),
            1
        );
        assert_ne!(entries.first(), Some(&TabEntry::Separator));
        assert_ne!(entries.last(), Some(&TabEntry::Separator));
    }
}
