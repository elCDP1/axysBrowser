use gtk::prelude::*;
use gtk::{Box, Button, Entry, Label, Orientation, Overlay, Separator};
use std::rc::Rc;

use super::background::build_waves_background;

pub fn build_newtab(on_search: Rc<dyn Fn(String)>, on_about: Rc<dyn Fn()>) -> Box {
    let page = Box::new(Orientation::Vertical, 0);

    page.set_hexpand(true);
    page.set_vexpand(true);

    let overlay = Overlay::new();

    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    // Shared vector-waves background (same design on new tab and privacy pages).
    let background = build_waves_background();

    overlay.set_child(Some(&background));

    /*
     * Central content.
     */
    let content = Box::new(Orientation::Vertical, 0);

    content.set_hexpand(true);
    content.set_vexpand(true);

    content.set_halign(gtk::Align::Center);

    content.set_valign(gtk::Align::Start);

    /*
     * Keep Axys clearly above the search field.
     */
    content.set_margin_top(62);

    let logo = Button::with_label("axys");

    logo.add_css_class("newtab-logo");

    logo.add_css_class("flat");

    logo.set_halign(gtk::Align::Center);

    logo.set_tooltip_text(Some(&rust_i18n::t!("about.tooltip")));

    /*
     * Bigger gap between logo and search bar.
     */
    logo.set_margin_bottom(120);

    {
        let on_about = on_about.clone();

        logo.connect_clicked(move |_| {
            on_about();
        });
    }

    /*
     * Wider and slightly taller search bar.
     */
    let search = Entry::builder()
        .placeholder_text(rust_i18n::t!("app.search_or_enter_url").as_ref())
        .hexpand(false)
        .width_chars(72)
        .max_width_chars(90)
        .halign(gtk::Align::Center)
        .build();

    search.add_css_class("search-main");

    /*
     * Push it down a little more.
     */
    search.set_margin_top(12);
    search.set_margin_bottom(10);

    gtk::prelude::EntryExt::set_alignment(&search, 0.5);

    {
        let on_search = on_search.clone();

        search.connect_activate(move |entry| {
            on_search(entry.text().to_string());
        });
    }

    content.append(&logo);
    content.append(&search);

    overlay.add_overlay(&content);

    // Make the overlay request enough height for the logo + search field,
    // otherwise the footer is drawn on top of them in small windows.
    overlay.set_measure_overlay(&content, true);

    /*
     * Bottom information.
     */
    let footer = Box::new(Orientation::Vertical, 6);

    footer.set_halign(gtk::Align::Center);

    footer.set_margin_top(12);
    footer.set_margin_bottom(12);

    let separator = Separator::new(Orientation::Horizontal);

    separator.set_opacity(0.25);

    let copyright = Label::new(Some(&format!(
        "{} · {}",
        rust_i18n::t!("about.copyright"),
        env!("CARGO_PKG_VERSION")
    )));

    copyright.add_css_class("dim-label");

    let links = Box::new(Orientation::Horizontal, 4);

    links.set_halign(gtk::Align::Center);

    for (label_key, target) in [
        ("about.title", "axys://about"),
        ("privacy.title", "axys://privacy"),
        ("settings.title", "axys://settings"),
    ] {
        let link = Button::with_label(&rust_i18n::t!(label_key));

        link.add_css_class("flat");

        let on_search = on_search.clone();

        link.connect_clicked(move |_| {
            on_search(target.to_string());
        });

        links.append(&link);
    }

    footer.append(&separator);

    footer.append(&copyright);

    footer.append(&links);

    page.append(&overlay);
    page.append(&footer);

    page
}
