use gtk::prelude::*;
use gtk::{Box, Button, DropDown, Label, Orientation, Separator};

pub fn build_welcome() -> Box {
    let page = Box::new(Orientation::Vertical, 18);

    page.set_vexpand(true);
    page.set_hexpand(true);
    page.set_valign(gtk::Align::Center);
    page.set_halign(gtk::Align::Center);

    let title = Label::new(Some("Welcome to axysBrowser"));
    title.add_css_class("title-1");

    let subtitle = Label::new(Some("A lightweight, privacy-focused browser for Linux."));

    subtitle.add_css_class("dim-label");

    let section = Box::new(Orientation::Vertical, 8);

    let search_label = Label::new(Some("Search engine"));
    search_label.set_halign(gtk::Align::Start);

    let search = DropDown::from_strings(&["Brave Search", "DuckDuckGo", "Google", "Bing"]);

    search.set_selected(0);

    let notice = Label::new(Some("Brave Search is the default search engine."));

    notice.add_css_class("dim-label");
    notice.set_wrap(true);
    notice.set_max_width_chars(52);

    section.append(&search_label);
    section.append(&search);
    section.append(&notice);

    let separator = Separator::new(Orientation::Horizontal);

    let description = Label::new(Some(
        "axysBrowser is designed to be simple, lightweight and privacy-focused,\
        with a native Linux interface and a clean browsing experience.",
    ));

    description.set_wrap(true);
    description.set_max_width_chars(54);
    description.set_justify(gtk::Justification::Center);

    let continue_button = Button::with_label("Continue");
    continue_button.add_css_class("suggested-action");
    continue_button.set_halign(gtk::Align::Center);

    page.append(&title);
    page.append(&subtitle);
    page.append(&section);
    page.append(&separator);
    page.append(&description);
    page.append(&continue_button);

    page
}
