use gtk::prelude::*;
use gtk::{Box, Button, Label, Orientation, Overlay};
use std::rc::Rc;

use super::background::build_waves_background;

/// Privacy landing page: same look as the new tab page (shared waves
/// background), with just a short private-mode heading and a continue
/// button. Deliberately minimal — no long warning text.
pub fn build_privacy(on_continue: Rc<dyn Fn(String)>) -> Box {
    let page = Box::new(Orientation::Vertical, 0);

    page.set_hexpand(true);
    page.set_vexpand(true);

    let overlay = Overlay::new();

    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    overlay.set_child(Some(&build_waves_background()));

    let content = Box::new(Orientation::Vertical, 10);

    content.set_hexpand(true);
    content.set_vexpand(true);

    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);

    let title = Label::new(Some(&rust_i18n::t!("privacy.title")));
    title.add_css_class("title-1");

    let subtitle = Label::new(Some(&rust_i18n::t!("privacy.subtitle")));
    subtitle.add_css_class("dim-label");
    subtitle.set_wrap(true);
    subtitle.set_max_width_chars(48);
    subtitle.set_justify(gtk::Justification::Center);

    let continue_button = Button::with_label(&rust_i18n::t!("privacy.continue"));
    continue_button.add_css_class("suggested-action");
    continue_button.set_halign(gtk::Align::Center);
    continue_button.set_margin_top(8);

    continue_button.connect_clicked(move |_| {
        on_continue("axys://newtab".to_string());
    });

    content.append(&title);
    content.append(&subtitle);
    content.append(&continue_button);

    overlay.add_overlay(&content);
    overlay.set_measure_overlay(&content, true);

    page.append(&overlay);

    page
}
