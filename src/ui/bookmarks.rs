use gtk::prelude::*;
use gtk::{Align, Box, Button, Image, Label, MenuButton, Orientation, Popover};
use std::cell::Cell;
use std::rc::Rc;

use webkit6::gdk;

use crate::app_state::AppState;
use crate::browser::bookmarks::Bookmark;

pub struct BookmarkBar {
    pub root: Box,
    refresh: Rc<dyn Fn()>,
}

impl BookmarkBar {
    pub fn new(state: Rc<AppState>, on_open: Rc<dyn Fn(String)>) -> Self {
        let root = Box::new(Orientation::Horizontal, 2);

        root.set_hexpand(true);
        root.set_visible(false);

        root.add_css_class("bookmark-bar");

        let items = Box::new(Orientation::Horizontal, 2);

        items.set_hexpand(true);
        items.set_vexpand(false);

        let overflow_button = MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("More bookmarks")
            .build();

        overflow_button.add_css_class("bookmark-overflow");

        overflow_button.set_visible(false);

        let overflow_popover = Popover::new();

        let overflow_list = Box::new(Orientation::Vertical, 4);

        overflow_list.set_margin_top(8);
        overflow_list.set_margin_bottom(8);
        overflow_list.set_margin_start(8);
        overflow_list.set_margin_end(8);

        overflow_popover.set_child(Some(&overflow_list));

        overflow_button.set_popover(Some(&overflow_popover));

        root.append(&items);

        root.append(&overflow_button);

        let reflow_scheduled = Rc::new(Cell::new(false));

        let schedule_reflow: Rc<dyn Fn()> = {
            let items = items.clone();

            let overflow_button = overflow_button.clone();

            let overflow_list = overflow_list.clone();

            let overflow_popover = overflow_popover.clone();

            let reflow_scheduled = reflow_scheduled.clone();

            let on_open = on_open.clone();

            Rc::new(move || {
                if reflow_scheduled.replace(true) {
                    return;
                }

                let items = items.clone();

                let overflow_button = overflow_button.clone();

                let overflow_list = overflow_list.clone();

                let overflow_popover = overflow_popover.clone();

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

                    for button in &buttons {
                        let button_width = button.width();

                        if button_width <= 0 {
                            continue;
                        }

                        if used + button_width + spacing <= width {
                            used += button_width + spacing;
                        } else {
                            button.set_visible(false);
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

                        let overflow_item = Button::with_label(&title);

                        overflow_item.add_css_class("bookmark-overflow-item");

                        overflow_item.set_halign(Align::Fill);

                        overflow_item.set_hexpand(true);

                        overflow_item.set_tooltip_text(Some(&url));

                        row.append(&overflow_item);

                        let popover = overflow_popover.clone();

                        let on_open = on_open.clone();

                        overflow_item.connect_clicked(move |_| {
                            popover.popdown();

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

            Rc::new(move || {
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
            let schedule_reflow = schedule_reflow.clone();

            items.connect_notify_local(Some("width"), move |_, _| {
                schedule_reflow();
            });
        }

        {
            let schedule_reflow = schedule_reflow.clone();

            root.connect_notify_local(Some("width"), move |_, _| {
                schedule_reflow();
            });
        }

        refresh();

        Self { root, refresh }
    }

    pub fn refresh(&self) {
        (self.refresh)();
    }
}

fn load_bookmark_icon(bookmark: &Bookmark) -> Image {
    if let Some(path) = bookmark.favicon_path.as_ref()
        && let Ok(texture) = gdk::Texture::from_filename(path)
    {
        return Image::from_paintable(Some(&texture));
    }

    Image::from_icon_name("web-browser-symbolic")
}
