use gtk::prelude::*;
use gtk::{Box, Label, Orientation};

pub fn build_about() -> Box {
    let page = Box::new(Orientation::Vertical, 16);

    page.set_vexpand(true);
    page.set_hexpand(true);
    page.set_valign(gtk::Align::Center);
    page.set_halign(gtk::Align::Center);

    let title = Label::new(Some("axysBrowser"));
    title.add_css_class("title-1");

    let version = Label::new(Some("beta-1.0"));
    version.add_css_class("dim-label");

    let description = Label::new(Some(
        "Linux-first web browser built with Rust, GTK4 and WebKitGTK.",
    ));

    description.set_wrap(true);
    description.set_max_width_chars(60);
    description.set_justify(gtk::Justification::Center);

    let author = Label::new(Some("Original project by elCDP1"));

    author.add_css_class("title-3");

    let copyright = Label::new(Some("Copyright © 2026 elCDP1"));

    copyright.add_css_class("dim-label");

    let license = Label::new(Some("AXYS Attribution License 1.0"));

    license.add_css_class("dim-label");

    let notice = Label::new(Some(
        "Derivative works may be modified and renamed, but must retain visible attribution to the original axysBrowser project and elCDP1.",
    ));

    notice.set_wrap(true);
    notice.set_max_width_chars(60);
    notice.set_justify(gtk::Justification::Center);
    notice.add_css_class("dim-label");

    page.append(&title);
    page.append(&version);
    page.append(&description);
    page.append(&author);
    page.append(&copyright);
    page.append(&license);
    page.append(&notice);

    page
}
