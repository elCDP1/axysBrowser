use gtk::prelude::*;
use gtk::{Box, Button, Label, Orientation};
use std::rc::Rc;

pub fn build_tools(
    on_downloads: Rc<dyn Fn()>,
    on_history: Rc<dyn Fn()>,
    on_clear_browsing_data: Rc<dyn Fn()>,
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

    let history = Button::with_label(&rust_i18n::t!("history.title"));

    history.set_halign(gtk::Align::Start);

    {
        let callback = on_history.clone();

        history.connect_clicked(move |_| {
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
            callback();

            status.set_text(&rust_i18n::t!("tools.cleared"));
        });
    }

    let note = Label::new(Some(&rust_i18n::t!("tools.note")));

    note.add_css_class("dim-label");

    note.set_halign(gtk::Align::Start);

    page.append(&title);

    page.append(&downloads);

    page.append(&history);

    page.append(&browsing_data);

    page.append(&status);

    page.append(&note);

    page
}
