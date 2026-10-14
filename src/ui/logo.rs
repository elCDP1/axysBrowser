use gtk::prelude::*;
use gtk::{Button, DrawingArea, Label, Orientation};
use std::f64::consts::PI;
use std::rc::Rc;

/// Integrated wave-mark that echoes the newtab background waves, so the
/// branding lives in the design itself instead of an image asset.
///
/// The accent is fixed (not theme-derived) so the mark reads well in both
/// light and dark mode without extra theme plumbing.
pub fn build_mark(px: i32) -> DrawingArea {
    let mark = DrawingArea::new();

    mark.set_content_width(px);
    mark.set_content_height(px * 3 / 4);

    mark.set_draw_func(move |_, cr, width, height| {
        let width = width.max(1) as f64;
        let height = height.max(1) as f64;

        for wave in 0..3 {
            let wave = wave as f64;

            cr.new_path();

            for step in 0..=120 {
                let t = step as f64 / 120.0;

                let x = t * width;

                let y = height * (0.30 + wave * 0.20)
                    + height * 0.14 * (t * PI * 2.0 + wave * 0.9).sin();

                if step == 0 {
                    cr.move_to(x, y);
                } else {
                    cr.line_to(x, y);
                }
            }

            cr.set_source_rgba(0.30, 0.55, 0.96, 0.55 + wave * 0.15);
            cr.set_line_width(2.0);
            cr.set_line_cap(gtk::cairo::LineCap::Round);

            let _ = cr.stroke();
        }
    });

    mark
}

/// Compact logo widget (mark + wordmark) for the tab bar.
pub fn build_logo_widget() -> gtk::Box {
    let row = gtk::Box::new(Orientation::Horizontal, 7);

    row.append(&build_mark(30));

    let word = Label::new(Some("axys"));

    word.add_css_class("logo-word");

    row.append(&word);

    row
}

/// About-button carrying the integrated logo.
pub fn build_logo_button(on_about: Rc<dyn Fn()>) -> Button {
    let button = Button::new();

    button.set_child(Some(&build_logo_widget()));

    button.add_css_class("flat");

    button.set_tooltip_text(Some(&rust_i18n::t!("about.tooltip")));

    button.connect_clicked(move |_| {
        on_about();
    });

    button
}
