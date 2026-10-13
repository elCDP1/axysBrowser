use gtk::prelude::*;
use gtk::{Box, Label, Orientation};

pub fn build_privacy() -> Box {
    let page = Box::new(Orientation::Vertical, 16);

    page.set_vexpand(true);
    page.set_hexpand(true);
    page.set_valign(gtk::Align::Center);
    page.set_halign(gtk::Align::Center);

    let title = Label::new(Some("Privacy mode"));

    title.add_css_class("title-1");

    let subtitle = Label::new(Some("A temporary browsing session"));

    subtitle.add_css_class("title-3");

    let description = Label::new(Some(
        "This window uses a separate ephemeral browsing session.\n\n\
                 Website data is not persisted when the privacy window closes.\n\n\
                 Brave Search is always used for searches in this mode.",
    ));

    description.set_wrap(true);

    description.set_justify(gtk::Justification::Center);

    description.set_max_width_chars(62);

    let note = Label::new(Some(
        "Privacy mode does not provide network anonymity. Your network connection is still handled normally.",
    ));

    note.add_css_class("dim-label");

    note.set_wrap(true);

    note.set_justify(gtk::Justification::Center);

    note.set_max_width_chars(62);

    page.append(&title);

    page.append(&subtitle);

    page.append(&description);

    page.append(&note);

    page
}
