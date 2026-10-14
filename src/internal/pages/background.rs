use gtk::DrawingArea;
use gtk::prelude::*;
use std::f64::consts::PI;

/// Shared vector-waves background (same design as the new tab page).
///
/// The drawing uses the real DrawingArea dimensions, so it automatically
/// scales to the current browser resolution.
pub fn build_waves_background() -> DrawingArea {
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

    background
}
