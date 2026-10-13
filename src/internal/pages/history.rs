use gtk::prelude::*;
use gtk::{Align, Box, Button, Entry, Label, Orientation, ScrolledWindow};
use std::rc::Rc;

use crate::browser::history::{HistoryEntry, HistoryManager};

fn build_entry_row(
    entry: &HistoryEntry,
    manager: &HistoryManager,
    on_open: &Rc<dyn Fn(String)>,
) -> Box {
    let row = Box::new(Orientation::Horizontal, 12);

    row.set_hexpand(true);

    let info = Box::new(Orientation::Vertical, 3);

    info.set_hexpand(true);

    let title = Label::new(Some(&entry.title));

    title.set_halign(Align::Start);

    title.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let url = Label::new(Some(&entry.url));

    url.add_css_class("dim-label");

    url.set_halign(Align::Start);

    url.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

    info.append(&title);
    info.append(&url);

    row.append(&info);

    let open = Button::with_label(&rust_i18n::t!("history.open"));

    open.add_css_class("flat");

    {
        let on_open = on_open.clone();

        let url = entry.url.clone();

        open.connect_clicked(move |_| {
            on_open(url.clone());
        });
    }

    row.append(&open);

    let remove = Button::with_label(&rust_i18n::t!("history.delete"));

    remove.add_css_class("flat");

    {
        let manager = manager.clone();

        let url = entry.url.clone();

        remove.connect_clicked(move |_| {
            manager.remove_url(&url);
        });
    }

    row.append(&remove);

    row
}

pub fn build_history(manager: HistoryManager, on_open: Rc<dyn Fn(String)>) -> Box {
    let page = Box::new(Orientation::Vertical, 18);

    page.set_margin_top(28);
    page.set_margin_bottom(28);
    page.set_margin_start(28);
    page.set_margin_end(28);

    page.set_vexpand(true);
    page.set_hexpand(true);

    let title = Label::new(Some(&rust_i18n::t!("history.title")));

    title.add_css_class("title-1");

    title.set_halign(Align::Start);

    let controls = Box::new(Orientation::Horizontal, 8);

    let search = Entry::builder()
        .placeholder_text(rust_i18n::t!("history.search").as_ref())
        .hexpand(true)
        .build();

    let clear = Button::with_label(&rust_i18n::t!("history.clear"));

    clear.add_css_class("flat");

    controls.append(&search);
    controls.append(&clear);

    let list = Box::new(Orientation::Vertical, 8);

    list.set_hexpand(true);

    let scroller = ScrolledWindow::new();

    scroller.set_child(Some(&list));

    scroller.set_hexpand(true);
    scroller.set_vexpand(true);

    page.append(&title);
    page.append(&controls);
    page.append(&scroller);

    let refresh: Rc<dyn Fn()> = {
        let list = list.clone();

        let manager = manager.clone();

        let search = search.clone();

        let on_open = on_open.clone();

        Rc::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }

            let entries = manager.search(&search.text());

            if entries.is_empty() {
                let empty = Label::new(Some(&rust_i18n::t!("history.empty")));

                empty.add_css_class("dim-label");

                empty.set_halign(Align::Start);

                list.append(&empty);

                return;
            }

            for entry in entries {
                list.append(&build_entry_row(&entry, &manager, &on_open));
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
        let manager = manager.clone();

        let refresh = refresh.clone();

        clear.connect_clicked(move |_| {
            manager.clear();

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
