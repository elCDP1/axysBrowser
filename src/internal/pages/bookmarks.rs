use gtk::prelude::*;
use gtk::{Align, Box, Button, Entry, Image, Label, Orientation, ScrolledWindow};
use std::rc::Rc;

use crate::browser::bookmarks::{Bookmark, BookmarkManager};

fn favicon(bookmark: &Bookmark) -> Image {
    if let Some(path) = bookmark.favicon_path.as_ref()
        && let Ok(texture) = webkit6::gdk::Texture::from_filename(path)
    {
        let image = Image::from_paintable(Some(&texture));

        image.set_pixel_size(16);

        return image;
    }

    let image = Image::from_icon_name("web-browser-symbolic");

    image.set_pixel_size(16);

    image
}

fn build_row(bookmark: &Bookmark, manager: &BookmarkManager, on_open: &Rc<dyn Fn(String)>) -> Box {
    let row = Box::new(Orientation::Horizontal, 10);

    row.set_hexpand(true);

    row.append(&favicon(bookmark));

    let info = Box::new(Orientation::Vertical, 3);

    info.set_hexpand(true);

    let title = Label::new(Some(&bookmark.title));

    title.set_halign(Align::Start);

    title.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let url = Label::new(Some(&bookmark.url));

    url.add_css_class("dim-label");

    url.set_halign(Align::Start);

    url.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

    info.append(&title);
    info.append(&url);

    row.append(&info);

    let open = Button::with_label(&rust_i18n::t!("bookmarks.open"));

    open.add_css_class("flat");

    {
        let on_open = on_open.clone();

        let url = bookmark.url.clone();

        open.connect_clicked(move |_| {
            on_open(url.clone());
        });
    }

    row.append(&open);

    let remove = Button::with_label(&rust_i18n::t!("bookmarks.remove"));

    remove.add_css_class("flat");

    {
        let manager = manager.clone();
        let url = bookmark.url.clone();

        remove.connect_clicked(move |_| {
            // The manager notifies subscribers, which rebuilds the list.
            manager.remove(&url);
        });
    }

    row.append(&remove);

    row
}

pub fn build_bookmarks(manager: BookmarkManager, on_open: Rc<dyn Fn(String)>) -> Box {
    let page = Box::new(Orientation::Vertical, 18);

    page.set_margin_top(28);
    page.set_margin_bottom(28);
    page.set_margin_start(28);
    page.set_margin_end(28);

    page.set_vexpand(true);
    page.set_hexpand(true);

    let title = Label::new(Some(&rust_i18n::t!("tools.bookmarks")));

    title.add_css_class("title-1");

    title.set_halign(Align::Start);

    let search = Entry::builder()
        .placeholder_text(rust_i18n::t!("bookmarks.search").as_ref())
        .hexpand(true)
        .build();

    let list = Box::new(Orientation::Vertical, 8);

    list.set_hexpand(true);

    let scroller = ScrolledWindow::new();

    scroller.set_child(Some(&list));

    scroller.set_hexpand(true);
    scroller.set_vexpand(true);

    page.append(&title);
    page.append(&search);
    page.append(&scroller);

    let refresh: Rc<dyn Fn()> = {
        let list = list.clone();

        let manager = manager.clone();

        // Weak: the entry owns the `changed` handler which owns `refresh`,
        // so a strong capture here would form a cycle that keeps the whole
        // page (and this listener) alive forever.
        let search = search.downgrade();

        let on_open = on_open.clone();

        Rc::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }

            let query = search
                .upgrade()
                .map(|entry| entry.text().trim().to_lowercase())
                .unwrap_or_default();

            let bookmarks: Vec<Bookmark> = manager
                .entries()
                .into_iter()
                .filter(|bookmark| {
                    query.is_empty()
                        || bookmark.title.to_lowercase().contains(&query)
                        || bookmark.url.to_lowercase().contains(&query)
                })
                .collect();

            if bookmarks.is_empty() {
                let message = if query.is_empty() {
                    rust_i18n::t!("bookmarks.empty")
                } else {
                    rust_i18n::t!("bookmarks.no_results")
                };

                let empty = Label::new(Some(&message));

                empty.add_css_class("dim-label");

                empty.set_halign(Align::Start);

                list.append(&empty);

                return;
            }

            for bookmark in bookmarks {
                list.append(&build_row(&bookmark, &manager, &on_open));
            }
        })
    };

    {
        let refresh = refresh.clone();

        search.connect_changed(move |_| {
            refresh();
        });
    }

    {
        let refresh = refresh.clone();

        manager.subscribe(&refresh);
    }

    refresh();

    page
}
