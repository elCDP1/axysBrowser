use gtk::prelude::*;
use gtk::{Box, Label, Orientation};

use crate::ui::logo::build_mark;

pub fn build_about() -> Box {
    let page = Box::new(Orientation::Vertical, 16);

    page.set_vexpand(true);
    page.set_hexpand(true);
    page.set_valign(gtk::Align::Center);
    page.set_halign(gtk::Align::Center);

    let mark = build_mark(72);

    mark.set_halign(gtk::Align::Center);

    let title = Label::new(Some("axysBrowser"));
    title.add_css_class("title-1");

    let version = Label::new(Some(&format!("axysBrowser {}", env!("CARGO_PKG_VERSION"))));
    version.add_css_class("dim-label");

    let description = Label::new(Some(&rust_i18n::t!("about.description")));

    description.set_wrap(true);
    description.set_max_width_chars(60);
    description.set_justify(gtk::Justification::Center);

    let author = Label::new(Some(&rust_i18n::t!("about.author")));

    author.add_css_class("title-3");

    let copyright = Label::new(Some(&rust_i18n::t!("about.copyright")));

    copyright.add_css_class("dim-label");

    let license = Label::new(Some("AXYS Attribution License 1.0"));

    license.add_css_class("dim-label");

    let notice = Label::new(Some(&rust_i18n::t!("about.notice")));

    notice.set_wrap(true);
    notice.set_max_width_chars(60);
    notice.set_justify(gtk::Justification::Center);
    notice.add_css_class("dim-label");

    page.append(&mark);

    page.append(&title);
    page.append(&version);
    page.append(&description);
    page.append(&author);
    page.append(&copyright);
    page.append(&license);
    page.append(&notice);

    page
}
