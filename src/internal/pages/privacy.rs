use gtk::prelude::*;
use gtk::{Box, Button, Label, Orientation};
use std::rc::Rc;

pub fn build_privacy(on_continue: Rc<dyn Fn(String)>) -> Box {
    let page = Box::new(Orientation::Vertical, 18);

    page.set_vexpand(true);
    page.set_hexpand(true);
    page.set_valign(gtk::Align::Center);
    page.set_halign(gtk::Align::Center);

    let title = Label::new(Some(&rust_i18n::t!("privacy.title")));
    title.add_css_class("title-1");

    let subtitle = Label::new(Some(&rust_i18n::t!("privacy.subtitle")));
    subtitle.add_css_class("dim-label");

    let description = Label::new(Some(&rust_i18n::t!("privacy.details")));
    description.set_wrap(true);
    description.set_max_width_chars(54);
    description.set_justify(gtk::Justification::Center);

    let note = Label::new(Some(&rust_i18n::t!("privacy.note")));
    note.add_css_class("dim-label");
    note.set_wrap(true);
    note.set_max_width_chars(52);
    note.set_justify(gtk::Justification::Center);

    let continue_button = Button::with_label(&rust_i18n::t!("privacy.continue"));
    continue_button.add_css_class("suggested-action");
    continue_button.set_halign(gtk::Align::Center);

    continue_button.connect_clicked(move |_| {
        on_continue("axys://newtab".to_string());
    });

    page.append(&title);
    page.append(&subtitle);
    page.append(&description);
    page.append(&note);
    page.append(&continue_button);

    page
}
