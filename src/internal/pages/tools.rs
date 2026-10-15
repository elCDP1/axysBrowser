use gtk::prelude::*;
use gtk::{Box, Button, Label, Orientation};
use std::rc::Rc;

use crate::browser::history::HistoryManager;

pub fn build_tools(
    on_downloads: Rc<dyn Fn()>,
    on_history: Rc<dyn Fn()>,
    on_clear_browsing_data: crate::app_state::ClearBrowsingData,
    history: HistoryManager,
) -> Box {
    let page = Box::new(Orientation::Vertical, 14);

    page.set_margin_top(28);
    page.set_margin_bottom(28);
    page.set_margin_start(28);
    page.set_margin_end(28);

    page.set_vexpand(true);
    page.set_hexpand(true);

    let title = Label::new(Some(&rust_i18n::t!("tools.title")));

    title.add_css_class("title-1");

    title.set_halign(gtk::Align::Start);

    let downloads = Button::with_label(&rust_i18n::t!("tools.open_downloads"));

    downloads.set_halign(gtk::Align::Start);

    {
        let callback = on_downloads.clone();

        downloads.connect_clicked(move |_| {
            callback();
        });
    }

    let history_button = Button::with_label(&rust_i18n::t!("history.title"));

    history_button.set_halign(gtk::Align::Start);

    {
        let callback = on_history.clone();

        history_button.connect_clicked(move |_| {
            callback();
        });
    }

    let browsing_data = Button::with_label(&rust_i18n::t!("tools.clear_browsing_data"));

    browsing_data.set_halign(gtk::Align::Start);

    browsing_data.add_css_class("destructive-action");

    let status = Label::new(None);

    status.add_css_class("dim-label");

    status.set_halign(gtk::Align::Start);

    {
        let callback = on_clear_browsing_data.clone();
        let status = status.clone();

        browsing_data.connect_clicked(move |_| {
            status.set_text(&rust_i18n::t!("tools.clearing"));

            let status = status.clone();

            callback(Rc::new(move |ok| {
                status.set_text(&rust_i18n::t!(if ok {
                    "tools.cleared"
                } else {
                    "tools.clear_failed"
                }));
            }));
        });
    }

    // Manual age-based history cleanup (the manager also caps the total at
    // MAX_HISTORY_ENTRIES, oldest first).
    let purge_title = Label::new(Some(&rust_i18n::t!("history.purge_title")));

    purge_title.add_css_class("title-3");

    purge_title.set_halign(gtk::Align::Start);

    let purge_row = Box::new(Orientation::Horizontal, 8);

    for (label_key, max_age_secs) in [
        ("history.older_day", 86_400u64),
        ("history.older_week", 604_800u64),
        ("history.older_month", 2_592_000u64),
    ] {
        let purge = Button::with_label(&rust_i18n::t!(label_key));

        purge.set_halign(gtk::Align::Start);

        {
            let history = history.clone();
            let status = status.clone();

            purge.connect_clicked(move |_| {
                let removed = history.remove_older_than(max_age_secs);

                status.set_text(&rust_i18n::t!("tools.purged", count = removed));
            });
        }

        purge_row.append(&purge);
    }

    let note = Label::new(Some(&rust_i18n::t!("tools.note")));

    note.add_css_class("dim-label");

    note.set_halign(gtk::Align::Start);

    page.append(&title);

    page.append(&downloads);

    page.append(&history_button);

    page.append(&browsing_data);

    page.append(&status);

    page.append(&purge_title);

    page.append(&purge_row);

    page.append(&note);

    page
}
