use gtk::DrawingArea;
use gtk::prelude::*;
use std::f64::consts::PI;

/// Colours of the wave background for one theme.
///
/// The geometry is shared by both themes; only these values change. The dark
/// veil was what made light pages look grey, so light mode gets a light one.
struct WavePalette {
    /// Base wash over the whole area.
    veil: (f64, f64, f64, f64),
    /// Main flowing waves (alpha grows with each wave).
    wave: (f64, f64, f64),
    wave_alpha: f64,
    wave_alpha_step: f64,
    /// Large secondary curves.
    secondary: (f64, f64, f64, f64),
    /// A few brighter accent curves.
    accent: (f64, f64, f64, f64),
}

fn wave_palette(dark: bool) -> WavePalette {
    if dark {
        WavePalette {
            veil: (0.10, 0.13, 0.18, 0.16),
            wave: (0.28, 0.47, 0.78),
            wave_alpha: 0.065,
            wave_alpha_step: 0.010,
            secondary: (0.08, 0.13, 0.22, 0.14),
            accent: (0.40, 0.60, 0.90, 0.085),
        }
    } else {
        WavePalette {
            veil: (0.62, 0.68, 0.78, 0.10),
            wave: (0.22, 0.42, 0.75),
            wave_alpha: 0.075,
            wave_alpha_step: 0.011,
            secondary: (0.32, 0.40, 0.55, 0.10),
            accent: (0.25, 0.48, 0.85, 0.11),
        }
    }
}

/// Whether the dark theme is the active one, told from the widget's resolved
/// foreground colour: a light foreground means the dark palette is in use.
///
/// Read at draw time rather than passed in when the page is built, so changing
/// the theme in Settings repaints the background without rebuilding the page.
fn is_dark_palette(foreground: gtk::gdk::RGBA) -> bool {
    let luminance = 0.2126 * foreground.red() as f64
        + 0.7152 * foreground.green() as f64
        + 0.0722 * foreground.blue() as f64;

    luminance > 0.5
}

/// Shared vector-waves background (same design as the new tab page).
///
/// The drawing uses the real DrawingArea dimensions, so it automatically
/// scales to the current browser resolution.
pub fn build_waves_background() -> DrawingArea {
    let background = DrawingArea::new();

    background.set_hexpand(true);
    background.set_vexpand(true);

    background.set_draw_func(|widget, cr, width, height| {
        let palette = wave_palette(is_dark_palette(widget.color()));

        let width = width.max(1) as f64;
        let height = height.max(1) as f64;

        /*
         * Base tint. Transparent so the Axys theme remains visible
         * underneath it, and light in light mode: a dark wash over a
         * near-white page was what made the internal pages look grey.
         */
        let (veil_r, veil_g, veil_b, veil_a) = palette.veil;

        cr.set_source_rgba(veil_r, veil_g, veil_b, veil_a);

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
            let (wave_r, wave_g, wave_b) = palette.wave;

            let alpha = palette.wave_alpha + wave * palette.wave_alpha_step;

            cr.set_source_rgba(wave_r, wave_g, wave_b, alpha);

            cr.set_line_width(1.0 + wave * 0.10);

            let _ = cr.stroke();
        }

        /*
         * Large secondary curves, slightly darker.
         */
        let (secondary_r, secondary_g, secondary_b, secondary_a) = palette.secondary;

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

            cr.set_source_rgba(secondary_r, secondary_g, secondary_b, secondary_a);

            cr.set_line_width(1.15);

            let _ = cr.stroke();
        }

        /*
         * A few brighter accent curves.
         */
        let (accent_r, accent_g, accent_b, accent_a) = palette.accent;

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

            cr.set_source_rgba(accent_r, accent_g, accent_b, accent_a);

            cr.set_line_width(1.0);

            let _ = cr.stroke();
        }
    });

    background
}

#[cfg(test)]
mod tests {
    use super::{is_dark_palette, wave_palette};
    use gtk::gdk::RGBA;

    #[test]
    fn a_light_foreground_means_the_dark_palette() {
        // The app's own palettes: #e9ebec on dark, #161a20 on light.
        assert!(is_dark_palette(RGBA::new(0.91, 0.92, 0.93, 1.0)));
        assert!(!is_dark_palette(RGBA::new(0.09, 0.10, 0.13, 1.0)));
    }

    #[test]
    fn the_light_theme_washes_the_page_with_a_light_veil() {
        let dark = wave_palette(true);
        let light = wave_palette(false);

        let brightness = |veil: (f64, f64, f64, f64)| veil.0 + veil.1 + veil.2;

        assert!(
            brightness(light.veil) > brightness(dark.veil),
            "light mode must not darken the page"
        );
        // Low enough that the theme colour still shows through.
        assert!(light.veil.3 <= 0.15);
        assert!(dark.veil.3 <= 0.20);
    }

    #[test]
    fn the_waves_stay_visible_in_both_themes() {
        for dark in [true, false] {
            let palette = wave_palette(dark);

            assert!(palette.wave_alpha > 0.0);
            assert!(palette.secondary.3 > 0.0);
            assert!(palette.accent.3 > 0.0);
        }
    }
}
