use gtk::prelude::*;
use gtk::{Box, Button, DrawingArea, Entry, Label, Orientation, Overlay, Separator};
use std::f64::consts::PI;
use std::rc::Rc;

pub fn build_newtab(on_search: Rc<dyn Fn(String)>, on_about: Rc<dyn Fn()>) -> Box {
    let page = Box::new(Orientation::Vertical, 0);

    page.set_hexpand(true);
    page.set_vexpand(true);

    let overlay = Overlay::new();

    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    /*
     * Vector background.
     *
     * The drawing uses the real DrawingArea dimensions, so it
     * automatically scales to the current browser resolution.
     */
    let background = DrawingArea::new();

    background.set_hexpand(true);
    background.set_vexpand(true);

    background.set_draw_func(|_, cr, width, height| {
        let width = width.max(1) as f64;
        let height = height.max(1) as f64;

        /*
         * Dark base tint. This is intentionally transparent so
         * the Axys theme remains visible underneath it.
         */
        cr.set_source_rgba(0.10, 0.13, 0.18, 0.16);

        cr.rectangle(0.0, 0.0, width, height);

        let _ = cr.fill();

        /*
         * Main flowing waves.
         *
         * They cross the complete screen instead of being limited
         * to the centre, which makes the background visible even
         * behind the logo and search area.
         */
        for wave in 0..9 {
            let wave = wave as f64;

            cr.new_path();

            let vertical_offset = height * 0.06 * wave;

            let amplitude = height * (0.075 + wave * 0.004);

            let phase = wave * 0.42;

            for step in 0..=240 {
                let t = step as f64 / 240.0;

                let x = t * width;

                let primary = (t * PI * 2.4 + phase).sin();

                let secondary = (t * PI * 5.0 + phase * 0.7).sin() * 0.18;

                let y = height * 0.43 + vertical_offset + amplitude * (primary + secondary);

                if step == 0 {
                    cr.move_to(x, y);
                } else {
                    cr.line_to(x, y);
                }
            }

            /*
             * Strong enough to be visible in both themes.
             */
            let alpha = 0.065 + wave * 0.010;

            cr.set_source_rgba(0.28, 0.47, 0.78, alpha);

            cr.set_line_width(1.0 + wave * 0.10);

            let _ = cr.stroke();
        }

        /*
         * Large secondary curves, slightly darker.
         */
        for wave in 0..6 {
            let wave = wave as f64;

            cr.new_path();

            let phase = wave * 0.75;

            for step in 0..=240 {
                let t = step as f64 / 240.0;

                let x = t * width;

                let y = height * 0.66
                    + height * 0.13 * (t * PI * 1.45 + phase).sin()
                    + wave * height * 0.025;

                if step == 0 {
                    cr.move_to(x, y);
                } else {
                    cr.line_to(x, y);
                }
            }

            cr.set_source_rgba(0.08, 0.13, 0.22, 0.14);

            cr.set_line_width(1.15);

            let _ = cr.stroke();
        }

        /*
         * A few brighter accent curves.
         */
        for wave in 0..3 {
            let wave = wave as f64;

            cr.new_path();

            for step in 0..=240 {
                let t = step as f64 / 240.0;

                let x = t * width;

                let y = height * 0.28
                    + height * 0.10 * (t * PI * 1.8 + wave * 0.9).sin()
                    + wave * height * 0.035;

                if step == 0 {
                    cr.move_to(x, y);
                } else {
                    cr.line_to(x, y);
                }
            }

            cr.set_source_rgba(0.40, 0.60, 0.90, 0.085);

            cr.set_line_width(1.0);

            let _ = cr.stroke();
        }
    });

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

    logo.set_tooltip_text(Some("About axysBrowser"));

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
        .placeholder_text("Search or enter URL")
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

    /*
     * Bottom information.
     */
    let footer = Box::new(Orientation::Vertical, 6);

    footer.set_halign(gtk::Align::Center);

    footer.set_margin_top(12);
    footer.set_margin_bottom(12);

    let separator = Separator::new(Orientation::Horizontal);

    separator.set_opacity(0.25);

    let copyright = Label::new(Some("© 2026 axysBrowser contributors · beta-1.0"));

    copyright.add_css_class("dim-label");

    let links = Label::new(Some("About · Privacy · Settings"));

    links.add_css_class("dim-label");

    footer.append(&separator);

    footer.append(&copyright);

    footer.append(&links);

    page.append(&overlay);
    page.append(&footer);

    page
}
