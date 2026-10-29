use gtk::prelude::*;
use gtk::{Align, Box, Button, Image, Label, Orientation};
use std::cell::Cell;
use std::rc::Rc;

use webkit6::gdk;

use crate::app_state::AppState;
use crate::browser::bookmarks::Bookmark;

pub struct BookmarkBar {
    pub root: Box,
    /// Overflow panel owned here but parented by the window overlay.
    pub overflow_panel: Box,
    on_panel_toggled: Rc<dyn Fn()>,
    refresh: Rc<dyn Fn()>,
}

impl BookmarkBar {
    pub fn new(
        state: Rc<AppState>,
        on_open: Rc<dyn Fn(String)>,
        on_panel_toggled: Rc<dyn Fn()>,
    ) -> Self {
        let root = Box::new(Orientation::Horizontal, 2);

        root.set_hexpand(true);
        root.set_visible(false);

        root.add_css_class("bookmark-bar");

        let items = Box::new(Orientation::Horizontal, 2);

        items.set_hexpand(true);
        items.set_vexpand(false);

        let overflow_button = Button::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text(rust_i18n::t!("bookmarks.more").as_ref())
            .build();

        overflow_button.add_css_class("bookmark-overflow");

        overflow_button.set_visible(false);

        // Overflow panel: plain in-window box (see ui::menu).
        let overflow_panel = Box::new(Orientation::Vertical, 0);

        overflow_panel.add_css_class("menu-panel");

        overflow_panel.set_visible(false);

        let overflow_list = Box::new(Orientation::Vertical, 4);

        overflow_list.set_margin_top(8);
        overflow_list.set_margin_bottom(8);
        overflow_list.set_margin_start(8);
        overflow_list.set_margin_end(8);
        overflow_list.set_size_request(280, -1);

        overflow_panel.append(&overflow_list);

        {
            let overflow_panel = overflow_panel.clone();
            let on_panel_toggled = on_panel_toggled.clone();

            overflow_button.connect_clicked(move |_| {
                overflow_panel.set_visible(!overflow_panel.is_visible());
                on_panel_toggled();
            });
        }

        root.append(&items);

        root.append(&overflow_button);

        let reflow_scheduled = Rc::new(Cell::new(false));

        let schedule_reflow: Rc<dyn Fn()> = {
            let items = items.clone();

            let overflow_button = overflow_button.clone();

            let overflow_list = overflow_list.clone();

            let overflow_panel = overflow_panel.clone();

            let on_panel_toggled = on_panel_toggled.clone();

            let reflow_scheduled = reflow_scheduled.clone();

            let on_open = on_open.clone();

            Rc::new(move || {
                if reflow_scheduled.replace(true) {
                    return;
                }

                let items = items.clone();

                let overflow_button = overflow_button.clone();

                let overflow_list = overflow_list.clone();

                let overflow_panel = overflow_panel.clone();

                let on_panel_toggled = on_panel_toggled.clone();

                let reflow_scheduled = reflow_scheduled.clone();

                let on_open = on_open.clone();

                gtk::glib::idle_add_local_once(move || {
                    reflow_scheduled.set(false);

                    let width = items.width();

                    if width <= 0 {
                        return;
                    }

                    let spacing = 8;

                    let buttons = items
                        .observe_children()
                        .snapshot()
                        .iter()
                        .filter_map(|child| child.clone().downcast::<Button>().ok())
                        .collect::<Vec<_>>();

                    for button in &buttons {
                        button.set_visible(true);
                    }

                    let mut used = 0;

                    let mut overflowing = false;

                    for button in &buttons {
                        let button_width = button.width();

                        if button_width <= 0 {
                            continue;
                        }

                        // Hide a contiguous suffix (Chromium-like): once one
                        // item no longer fits, every later one overflows too,
                        // instead of leaving narrow items floating after the
                        // hidden ones.
                        if overflowing || used + button_width + spacing > width {
                            overflowing = true;
                            button.set_visible(false);
                        } else {
                            used += button_width + spacing;
                        }
                    }

                    let hidden = buttons
                        .iter()
                        .filter(|button| !button.is_visible())
                        .cloned()
                        .collect::<Vec<_>>();

                    overflow_button.set_visible(!hidden.is_empty());

                    while let Some(child) = overflow_list.first_child() {
                        overflow_list.remove(&child);
                    }

                    for button in hidden {
                        let title = button
                            .child()
                            .and_then(|child| child.downcast::<Box>().ok())
                            .and_then(|box_widget| box_widget.last_child())
                            .and_then(|child| child.downcast::<Label>().ok())
                            .map(|label| label.text().to_string())
                            .unwrap_or_default();

                        let url = button
                            .tooltip_text()
                            .map(|text| text.to_string())
                            .unwrap_or_default();

                        let row = Box::new(Orientation::Horizontal, 6);

                        let icon = button
                            .child()
                            .and_then(|child| child.downcast::<Box>().ok())
                            .and_then(|box_widget| box_widget.first_child())
                            .and_then(|child| child.downcast::<Image>().ok());

                        if let Some(icon) = icon {
                            row.append(&icon);
                        }

                        let overflow_item = Button::new();

                        overflow_item.add_css_class("bookmark-overflow-item");

                        overflow_item.set_halign(Align::Fill);

                        overflow_item.set_hexpand(true);

                        overflow_item.set_tooltip_text(Some(&url));

                        // Custom label (instead of with_label) so long titles
                        // ellipsize instead of being clipped by the popover.
                        {
                            let item_label = Label::new(Some(&title));

                            item_label.set_halign(Align::Start);

                            item_label.set_hexpand(true);

                            item_label.set_ellipsize(gtk::pango::EllipsizeMode::End);

                            item_label.set_max_width_chars(30);

                            overflow_item.set_child(Some(&item_label));
                        }

                        row.append(&overflow_item);

                        let overflow_panel = overflow_panel.clone();

                        let on_panel_toggled = on_panel_toggled.clone();

                        let on_open = on_open.clone();

                        overflow_item.connect_clicked(move |_| {
                            overflow_panel.set_visible(false);
                            on_panel_toggled();

                            on_open(url.clone());
                        });

                        overflow_list.append(&row);
                    }
                });
            })
        };

        let refresh: Rc<dyn Fn()> = {
            let root = root.clone();

            let items = items.clone();

            let overflow_button = overflow_button.clone();

            let overflow_list = overflow_list.clone();

            let manager = state.bookmarks.clone();

            let schedule_reflow = schedule_reflow.clone();

            let on_open = on_open.clone();

            // Last revision rebuilt here. `refresh` is also called on
            // navigation and tab switches, where the list never changed.
            let last_revision: Rc<Cell<Option<u64>>> = Rc::new(Cell::new(None));

            Rc::new(move || {
                // Static tooltip follows the UI language (titles come from
                // the bookmarks themselves).
                overflow_button.set_tooltip_text(Some(&rust_i18n::t!("bookmarks.more")));

                let revision = manager.revision();

                if last_revision.get() == Some(revision) {
                    return;
                }

                last_revision.set(Some(revision));

                while let Some(child) = items.first_child() {
                    items.remove(&child);
                }

                while let Some(child) = overflow_list.first_child() {
                    overflow_list.remove(&child);
                }

                let bookmarks = manager.entries();

                if bookmarks.is_empty() {
                    root.set_visible(false);

                    overflow_button.set_visible(false);

                    return;
                }

                root.set_visible(true);

                overflow_button.set_visible(false);

                for bookmark in bookmarks {
                    let button = Button::new();

                    button.add_css_class("bookmark-button");

                    button.set_hexpand(false);

                    button.set_tooltip_text(Some(&bookmark.url));

                    let content = Box::new(Orientation::Horizontal, 6);

                    let icon = load_bookmark_icon(&bookmark);

                    icon.set_pixel_size(16);

                    content.append(&icon);

                    let label = Label::new(Some(&bookmark.title));

                    label.set_halign(Align::Start);

                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

                    content.append(&label);

                    button.set_child(Some(&content));

                    let url = bookmark.url.clone();

                    let on_open = on_open.clone();

                    button.connect_clicked(move |_| {
                        on_open(url.clone());
                    });

                    items.append(&button);
                }

                schedule_reflow();
            })
        };

        state.bookmarks.subscribe(&refresh);

        {
            // Weak: `schedule_reflow` captures `items` (its own child), so a
            // strong capture here would be an `items -> handler ->
            // schedule_reflow -> items` cycle that keeps the bar (and the
            // `navigate_tab` it holds) alive after the window closes.
            let schedule_reflow = Rc::downgrade(&schedule_reflow);

            items.connect_notify_local(Some("width"), move |_, _| {
                if let Some(schedule_reflow) = schedule_reflow.upgrade() {
                    schedule_reflow();
                }
            });
        }

        {
            let schedule_reflow = Rc::downgrade(&schedule_reflow);

            root.connect_notify_local(Some("width"), move |_, _| {
                if let Some(schedule_reflow) = schedule_reflow.upgrade() {
                    schedule_reflow();
                }
            });
        }

        refresh();

        Self {
            root,
            overflow_panel,
            on_panel_toggled,
            refresh,
        }
    }

    pub fn refresh(&self) {
        (self.refresh)();
    }

    /// Hides the overflow panel and syncs the window catcher.
    pub fn close_panels(&self) {
        self.overflow_panel.set_visible(false);
        (self.on_panel_toggled)();
    }
}

fn load_bookmark_icon(bookmark: &Bookmark) -> Image {
    // Favicons are decoded only when the bar actually rebuilds: the
    // revision check above skips navigation-driven refreshes, and resizes
    // only reflow. A cache would buy nothing and could serve a stale icon
    // after an edit.
    if let Some(path) = bookmark.favicon_path.as_ref()
        && let Ok(texture) = gdk::Texture::from_filename(path)
    {
        return Image::from_paintable(Some(&texture));
    }

    Image::from_icon_name("web-browser-symbolic")
}
