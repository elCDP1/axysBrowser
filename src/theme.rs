pub fn stylesheet(dark: bool) -> String {
    let palette = if dark { DARK_PALETTE } else { LIGHT_PALETTE };

    let dim_alpha = if dark { "0.62" } else { "0.72" };

    let border_alpha = if dark { "0.10" } else { "0.20" };

    let subtle_border_alpha = if dark { "0.08" } else { "0.17" };

    // The toolbar row sits on the palette background in dark mode. The tab
    // strip keeps the lifted chrome surface above it.
    let chrome_background = if dark { "@axys_bg" } else { "@axys_chrome" };

    // The dark toolbar is already a lifted surface, so a translucent white
    // address field read as flat grey. Use the palette background instead;
    // focus still raises it to `@axys_surface`. Light mode is unchanged.
    let address_background = if dark {
        "@axys_bg"
    } else {
        "alpha(@axys_fg, 0.045)"
    };

    // Toolbar action buttons keep their flat hit areas, but in dark mode they
    // sit on the same palette background as the address field instead of the
    // old translucent grey. Light mode keeps its existing backgrounds.
    let toolbar_button_background = if dark { "@axys_bg" } else { "transparent" };

    let nav_cluster_background = if dark {
        "@axys_bg"
    } else {
        "alpha(@axys_fg, 0.035)"
    };

    // The toolbar's GTK class can still carry the theme's own background,
    // which paints over the browser-chrome parent. Clear it only in dark mode
    // so the whole row matches the palette; light mode keeps its theme look.
    let toolbar_row_style = if dark {
        "background: @axys_bg; border: none; box-shadow: none;"
    } else {
        ""
    };

    format!(
        r#"
        {palette}

        window {{
            background-color: @axys_bg;
            color: @axys_fg;
        }}

        /* The tab strip and toolbar read as one modern browser chrome,
         * slightly lifted from the page content. */
        .browser-chrome {{
            background-color: {chrome_background};
            border-bottom: 1px solid alpha(@axys_fg, {subtle_border_alpha});
        }}

        /* The tab strip stays on the lifted chrome surface in both modes;
         * only the toolbar row below follows the palette background. */
        .tab-row {{
            background-color: @axys_chrome;
        }}

        headerbar,
        .titlebar {{
            background-color: @axys_chrome;
            background-image: none;
            color: @axys_fg;
            border-bottom: none;
            box-shadow: none;
        }}

        headerbar .title {{
            font-weight: 600;
        }}

        /* Native CSD buttons: keep the WM behavior and the same X / _ / □
         * glyphs, but give them the flatter modern chrome treatment. */
        .window-controls {{
            margin-left: 6px;
            padding-left: 6px;
            border-left: 1px solid alpha(@axys_fg, {subtle_border_alpha});
        }}

        windowcontrols > button,
        headerbar button.titlebutton,
        .window-controls > button {{
            min-width: 28px;
            min-height: 28px;
            margin: 2px 1px;
            padding: 0;
            border-radius: 9px;
            border: 1px solid transparent;
            background: transparent;
            background-image: none;
            box-shadow: none;
            color: alpha(@axys_fg, 0.82);
        }}

        windowcontrols > button:hover,
        headerbar button.titlebutton:hover,
        .window-controls > button:hover {{
            background-color: alpha(@axys_fg, 0.10);
            color: @axys_fg;
        }}

        windowcontrols > button:active,
        headerbar button.titlebutton:active,
        .window-controls > button:active {{
            background-color: alpha(@axys_fg, 0.16);
            color: @axys_fg;
        }}

        windowcontrols > button.close:hover,
        headerbar button.titlebutton.close:hover,
        .window-controls > button.window-close:hover {{
            background-color: #d94a4a;
            color: #ffffff;
        }}

        windowcontrols > button.close:active,
        headerbar button.titlebutton.close:active,
        .window-controls > button.window-close:active {{
            background-color: #bf3b3b;
            color: #ffffff;
        }}

        label {{
            color: @axys_fg;
        }}

        /* Hover tooltips must follow the theme too: the global `label`
         * rule above would otherwise paint dark text on the default dark
         * tooltip background in light mode (unreadable). */
        tooltip,
        tooltip.background {{
            background-color: @axys_bg;
            color: @axys_fg;
            border: 1px solid alpha(@axys_fg, {border_alpha});
            border-radius: 8px;
        }}

        tooltip label {{
            color: @axys_fg;
        }}

        .dim-label {{
            color: alpha(@axys_fg, {dim_alpha});
        }}

        .title-1,
        .title-2,
        .title-3,
        .title-4 {{
            color: @axys_fg;
        }}

        button,
        button image,
        button.flat,
        button.flat image {{
            color: @axys_fg;
        }}

        /* Plain buttons: subtle but visible in both modes. Class-based
         * buttons below (.flat, .tab-select, ...) override these. */
        button {{
            background-color: alpha(@axys_fg, 0.055);
            border: 1px solid alpha(@axys_fg, 0.12);
            border-radius: 9px;
        }}

        button:hover {{
            background-color: alpha(@axys_fg, 0.10);
        }}

        button:active {{
            background-color: alpha(@axys_fg, 0.15);
        }}

        button:disabled {{
            opacity: 0.45;
        }}

        button.flat {{
            background-color: transparent;
            border-color: transparent;
        }}

        button.suggested-action {{
            background-color: @axys_accent;
            border-color: transparent;
            color: #ffffff;
        }}

        button.suggested-action:hover {{
            background-color: alpha(@axys_accent, 0.88);
        }}

        button.suggested-action:active {{
            background-color: alpha(@axys_accent, 0.78);
        }}

        /* The global `label` colour rule overrides the colour inherited from
         * the button, so the white text of accent/destructive buttons must
         * be set on the label explicitly. */
        button.suggested-action label,
        button.destructive-action label {{
            color: #ffffff;
        }}

        button.destructive-action {{
            background-color: #c03636;
            border-color: transparent;
            color: #ffffff;
        }}

        button.destructive-action:hover {{
            background-color: alpha(#c03636, 0.88);
        }}

        button.destructive-action:active {{
            background-color: alpha(#c03636, 0.78);
        }}

        entry {{
            background-color: alpha(@axys_fg, 0.045);
            color: @axys_fg;
            border: 1px solid alpha(@axys_fg, {subtle_border_alpha});
            border-radius: 9px;
            padding: 5px 10px;
        }}

        entry:focus {{
            border-color: alpha(@axys_accent, 0.45);
        }}

        entry.error {{
            border-color: #c03636;
        }}

        label.error {{
            color: #e5534b;
            font-weight: 600;
        }}

        entry selection {{
            background-color: alpha(@axys_accent, 0.35);
            color: @axys_fg;
        }}

        popover,
        popover.background,
        popover > contents,
        popover.menu,
        popover.menu.contents,
        popover contents,
        window.popup,
        window.popup > contents {{
            background-color: @axys_bg;
            color: @axys_fg;
        }}

        popover *,
        window.popup * {{
            color: @axys_fg;
        }}

        popover separator,
        window.popup separator {{
            background-color: alpha(@axys_fg, 0.10);
            color: alpha(@axys_fg, 0.10);
        }}

        popover modelbutton,
        popover button.model,
        popover row,
        window.popup row {{
            background-color: transparent;
            color: @axys_fg;
        }}

        /* Compact menu rows so popups fit short windows instead of
         * spilling off-screen. */
        popover modelbutton,
        popover button.model {{
            min-height: 28px;
            padding-top: 3px;
            padding-bottom: 3px;
        }}

        switch {{
            min-height: 24px;
        }}

        .private-dot {{
            min-width: 10px;
            min-height: 10px;
            border-radius: 50%;
            background-color: #9b6bff;
        }}

        popover modelbutton:hover,
        popover button.model:hover,
        popover row:hover,
        window.popup row:hover {{
            background-color: alpha(@axys_fg, 0.08);
            color: @axys_fg;
        }}

        popover row:selected,
        window.popup row:selected,
        popover modelbutton:selected {{
            background-color: alpha(@axys_accent, 0.18);
            color: @axys_fg;
        }}

        dropdown {{
            color: @axys_fg;
            background-color: alpha(@axys_fg, 0.045);
            border: 1px solid alpha(@axys_fg, {border_alpha});
            border-radius: 9px;
        }}

        dropdown > button {{
            color: @axys_fg;
            background-color: transparent;
        }}

        dropdown > button:hover {{
            background-color: alpha(@axys_fg, 0.07);
        }}

        dropdown arrow {{
            color: @axys_fg;
        }}

        .address-top {{
            min-height: 34px;
            padding: 7px 15px;
            border-radius: 18px;
            border: 1px solid alpha(@axys_fg, {subtle_border_alpha});
            background: {address_background};
            box-shadow: none;
            font-size: 0.92em;
            color: @axys_fg;
        }}

        .address-top:focus {{
            border-color: alpha(@axys_accent, 0.65);
            background: @axys_surface;
            box-shadow: 0 0 0 3px alpha(@axys_accent, 0.14);
        }}

        .private-window .address-top,
        .private-window .address-top:focus {{
            border-color: rgba(155, 107, 255, 0.85);
        }}

        .private-window .tab.active {{
            box-shadow: inset 0 -2px 0 rgba(155, 107, 255, 0.95);
        }}

        .search-main {{
            min-height: 50px;
            padding: 12px 20px;
            border-radius: 25px;
            border: 1px solid alpha(@axys_fg, {border_alpha});
            background: @axys_surface;
            box-shadow: 0 10px 28px rgba(0, 0, 0, 0.14);
            font-size: 0.95em;
            color: @axys_fg;
        }}

        .search-main:focus {{
            border-color: alpha(@axys_accent, 0.65);
            box-shadow: 0 10px 28px rgba(0, 0, 0, 0.14), 0 0 0 3px alpha(@axys_accent, 0.14);
        }}

        .tab-strip {{
            padding: 1px 0;
        }}

        .tab {{
            min-height: 34px;
            padding: 2px 5px 2px 10px;
            border-radius: 10px;
            background: transparent;
            border: 1px solid transparent;
        }}

        .tab:hover {{
            background: alpha(@axys_fg, 0.055);
        }}

        .tab.active {{
            background: @axys_surface;
            border-color: alpha(@axys_fg, 0.10);
            box-shadow: 0 2px 7px rgba(0, 0, 0, 0.10);
        }}

        .tab-select {{
            padding: 2px 6px;
            border-radius: 7px;
            background: transparent;
            border: 1px solid transparent;
            box-shadow: none;
            color: @axys_fg;
        }}

        .tab.active .tab-select {{
            font-weight: 600;
        }}

        .tab-close {{
            opacity: 0.45;
            min-width: 24px;
            min-height: 24px;
            padding: 0;
            margin: 0;
            border: none;
            border-radius: 50%;
        }}

        .tab:hover .tab-close,
        .tab.active .tab-close {{
            opacity: 0.9;
        }}

        .newtab-logo {{
            padding: 10px 18px;
            border-radius: 18px;
            background: transparent;
            box-shadow: none;
            color: @axys_fg;
        }}

        .newtab-logo:hover {{
            background: alpha(@axys_fg, 0.055);
        }}

        .newtab-logo:active {{
            background: alpha(@axys_fg, 0.09);
        }}

        .newtab-word {{
            font-size: 3.75em;
            font-weight: 700;
            color: @axys_fg;
        }}

        .logo-word {{
            font-weight: 800;
            font-size: 1.16em;
        }}

        button.flat {{
            min-width: 32px;
            min-height: 32px;
            padding: 5px;
            border-radius: 9px;
            box-shadow: none;
        }}

        .toolbar button.flat {{
            background: {toolbar_button_background};
        }}

        button.flat:hover {{
            background: alpha(@axys_fg, 0.07);
        }}

        button.flat:active {{
            background: alpha(@axys_fg, 0.12);
        }}

        button.logo-button {{
            min-width: 0;
            min-height: 0;
            padding: 4px 8px;
            border-radius: 10px;
        }}

        button.logo-button:hover {{
            background: alpha(@axys_accent, 0.10);
        }}

        .nav-cluster {{
            padding: 2px;
            border-radius: 12px;
            background: {nav_cluster_background};
            border: 1px solid alpha(@axys_fg, {subtle_border_alpha});
        }}

        .nav-cluster button {{
            min-width: 32px;
            min-height: 32px;
            padding: 4px;
            border-radius: 9px;
        }}

        .toolbar {{
            padding: 0 2px;
            {toolbar_row_style}
        }}

        .toolbar-actions {{
            padding-left: 4px;
        }}

        button.new-tab {{
            min-width: 34px;
            min-height: 34px;
            padding: 5px;
            border-radius: 50%;
            background: alpha(@axys_accent, 0.10);
            border-color: transparent;
        }}

        button.new-tab:hover {{
            background: alpha(@axys_accent, 0.18);
        }}

        button.tab-close {{
            min-width: 24px;
            min-height: 24px;
            padding: 0;
            border-radius: 50%;
        }}

        button.tab-close:hover {{
            background: alpha(@axys_fg, 0.12);
        }}

        button.tab-select:hover {{
            background: transparent;
        }}

        .findbar {{
            padding: 6px 10px;
            border-radius: 12px;
            background-color: @axys_bg;
            color: @axys_fg;
            border: 1px solid alpha(@axys_fg, {border_alpha});
            box-shadow: 0 4px 18px rgba(0, 0, 0, 0.35);
        }}

        .findbar entry {{
            min-height: 30px;
            padding-top: 5px;
            padding-bottom: 5px;
        }}

        /* In-window menu panels (see ui::menu): plain widgets floating
         * above the page, so they can never end up behind it. */
        .menu-panel {{
            background-color: @axys_bg;
            color: @axys_fg;
            border: 1px solid alpha(@axys_fg, {border_alpha});
            border-radius: 12px;
            padding: 6px;
            box-shadow: 0 6px 24px rgba(0, 0, 0, 0.4);
        }}

        /* Panel scrolling viewports must not paint their own frame. */
        .panel-scroller {{
            background: none;
            border: none;
        }}

        /* Flat rows (hover highlight only), like a real menu: a plain
         * GtkButton would draw a bordered button per row. */
        .menu-row {{
            padding: 2px 12px;
            border-radius: 8px;
            min-height: 28px;
            background: none;
            background-image: none;
            border: 1px solid transparent;
            box-shadow: none;
        }}

        .menu-row:hover,
        .menu-row:focus-visible {{
            background-color: alpha(@axys_fg, 0.08);
        }}

        .menu-shortcut {{
            color: alpha(@axys_fg, 0.55);
            font-size: 0.9em;
        }}

        .menu-separator {{
            margin: 4px 6px;
            min-height: 1px;
            background-color: alpha(@axys_fg, 0.12);
        }}

        .menu-zoom-row {{
            padding: 2px 12px;
            min-height: 32px;
        }}

        .menu-zoom-button {{
            min-width: 30px;
            min-height: 26px;
            padding: 0 6px;
            border-radius: 8px;
            background: none;
            background-image: none;
            border: 1px solid alpha(@axys_fg, 0.18);
            box-shadow: none;
        }}

        .menu-zoom-button:hover {{
            background-color: alpha(@axys_fg, 0.08);
        }}

        .menu-zoom-value {{
            min-width: 44px;
            padding: 0 2px;
        }}

        .menu-row-selected {{
            background-color: alpha(@axys_fg, 0.10);
        }}

        .bookmark-bar {{
            padding: 5px 12px;
            margin: 0;
            min-height: 32px;
            border-spacing: 4px;
            border-top: 1px solid alpha(@axys_fg, {subtle_border_alpha});
            background-color: @axys_bg;
        }}

        .bookmark-button {{
            min-height: 26px;
            min-width: 0;
            padding: 4px 9px;
            margin: 2px 2px;
            border-radius: 8px;
            background-color: transparent;
            color: @axys_fg;
            border: 1px solid transparent;
        }}

        .bookmark-button:hover {{
            background-color: alpha(@axys_fg, 0.075);
            border-radius: 8px;
        }}

        .bookmark-button:active {{
            background-color: alpha(@axys_fg, 0.12);
        }}

        .bookmark-overflow {{
            min-height: 26px;
            min-width: 30px;
            padding: 4px 7px;
            margin: 2px 4px 2px 6px;
            border-radius: 9px;
            background-color: alpha(@axys_fg, 0.045);
            color: @axys_fg;
            border: 1px solid alpha(@axys_fg, {subtle_border_alpha});
        }}

        .bookmark-overflow:hover {{
            background-color: alpha(@axys_fg, 0.08);
        }}

        .bookmark-overflow-item {{
            min-height: 30px;
            padding: 6px 10px;
            margin: 1px 0;
            border-radius: 7px;
            background-color: transparent;
            color: @axys_fg;
        }}

        .bookmark-overflow-item:hover {{
            background-color: alpha(@axys_fg, 0.08);
        }}
        "#
    )
}

const DARK_PALETTE: &str = r#"
    @define-color axys_bg #1a1c20;
    @define-color axys_chrome #1e232c;
    @define-color axys_surface #282b31;
    @define-color axys_fg #eef2f7;
    @define-color axys_accent #5b9dff;
"#;

const LIGHT_PALETTE: &str = r#"
    @define-color axys_bg #f4f7fb;
    @define-color axys_chrome #ffffff;
    @define-color axys_surface #ffffff;
    @define-color axys_fg #16202c;
    @define-color axys_accent #2f6fe4;
"#;

#[cfg(test)]
mod tests {
    use super::{DARK_PALETTE, LIGHT_PALETTE};

    /// Reads `@define-color name #rrggbb;` out of a palette.
    fn color(palette: &str, name: &str) -> (u8, u8, u8) {
        let key = format!("@define-color {name} #");

        let line = palette
            .lines()
            .find(|line| line.trim_start().starts_with(&key))
            .unwrap_or_else(|| panic!("{name} is missing from the palette"));

        let hex: String = line.trim_start()[key.len()..].chars().take(6).collect();

        let byte = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&hex[range], 16).expect("palette colours are hex")
        };

        (byte(0..2), byte(2..4), byte(4..6))
    }

    #[test]
    fn both_palettes_define_every_theme_colour() {
        for palette in [DARK_PALETTE, LIGHT_PALETTE] {
            for name in [
                "axys_bg",
                "axys_chrome",
                "axys_surface",
                "axys_fg",
                "axys_accent",
            ] {
                let _ = color(palette, name);
            }
        }
    }

    #[test]
    fn internal_pages_are_near_white_in_light_mode() {
        let (r, g, b) = color(LIGHT_PALETTE, "axys_bg");

        // #e6e9ee read as grey next to the white toolbar; the pages must be
        // close to white while keeping a hint of colour.
        assert!(
            r >= 0xf0 && g >= 0xf0 && b >= 0xf0,
            "light background is too dark: #{r:02x}{g:02x}{b:02x}"
        );
        assert!(
            !(r == 0xff && g == 0xff && b == 0xff),
            "keep a hint of colour instead of pure white"
        );
    }

    #[test]
    fn the_chrome_and_surfaces_flip_with_the_theme() {
        let sum = |palette, name| {
            let (r, g, b) = color(palette, name);
            u32::from(r) + u32::from(g) + u32::from(b)
        };

        assert!(sum(LIGHT_PALETTE, "axys_chrome") > sum(DARK_PALETTE, "axys_chrome"));
        assert!(sum(LIGHT_PALETTE, "axys_surface") > sum(DARK_PALETTE, "axys_surface"));
    }

    #[test]
    fn the_dark_palette_stays_dark() {
        let (r, g, b) = color(DARK_PALETTE, "axys_bg");

        assert!(
            r < 0x60 && g < 0x60 && b < 0x60,
            "dark background is too light: #{r:02x}{g:02x}{b:02x}"
        );
    }

    #[test]
    fn the_toolbar_uses_the_dark_palette_background() {
        fn rule<'a>(css: &'a str, selector: &str) -> &'a str {
            let start = css.find(selector).expect("toolbar rule");
            let end = css[start..].find('}').expect("closed toolbar rule");

            &css[start..start + end]
        }

        let dark = super::stylesheet(true);
        let light = super::stylesheet(false);

        assert!(rule(&dark, ".browser-chrome {").contains("background-color: @axys_bg;"));
        assert!(rule(&dark, ".toolbar {").contains("background: @axys_bg;"));

        for selector in [".address-top {", ".nav-cluster {", ".toolbar button.flat {"] {
            assert!(
                rule(&dark, selector).contains("background: @axys_bg;"),
                "{selector} should use the dark palette background"
            );
        }

        assert!(rule(&light, ".address-top {").contains("background: alpha(@axys_fg, 0.045);"));
        assert!(rule(&light, ".nav-cluster {").contains("background: alpha(@axys_fg, 0.035);"));
        assert!(rule(&light, ".toolbar button.flat {").contains("background: transparent;"));
        assert!(rule(&light, ".browser-chrome {").contains("background-color: @axys_chrome;"));
        assert!(rule(&dark, ".tab-row {").contains("background-color: @axys_chrome;"));
        assert!(rule(&light, ".tab-row {").contains("background-color: @axys_chrome;"));
        assert!(!rule(&light, ".toolbar {").contains("background: @axys_bg;"));
    }

    #[test]
    fn the_dark_backgrounds_keep_only_a_slight_cool_tint() {
        // The window background and raised surfaces used to be clearly blue
        // (#171a21 / #262e39). Keep a cool hint, but not enough to make the
        // whole dark theme read as blue. The chrome stays as it is.
        for name in ["axys_bg", "axys_surface"] {
            let (r, g, b) = color(DARK_PALETTE, name);
            let spread = u8::max(u8::max(r, g), b) - u8::min(u8::min(r, g), b);

            assert!(spread <= 10, "{name} is too blue: #{r:02x}{g:02x}{b:02x}");
        }
    }
}
