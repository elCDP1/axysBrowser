pub fn stylesheet(dark: bool) -> String {
    let palette = if dark { DARK_PALETTE } else { LIGHT_PALETTE };

    let dim_alpha = if dark { "0.62" } else { "0.92" };

    let border_alpha = if dark { "0.10" } else { "0.15" };

    let subtle_border_alpha = if dark { "0.08" } else { "0.13" };

    format!(
        r#"
        {palette}

        window {{
            background-color: @axys_bg;
            color: @axys_fg;
        }}

        label {{
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
            min-height: 18px;
            padding: 5px 12px;
            border-radius: 16px;
            border: 1px solid alpha(@axys_fg, {subtle_border_alpha});
            background: alpha(@axys_fg, 0.045);
            box-shadow: none;
            font-size: 0.90em;
            color: @axys_fg;
        }}

        .address-top:focus {{
            border-color: alpha(@axys_accent, 0.45);
            background: alpha(@axys_fg, 0.07);
        }}

        .search-main {{
            min-height: 34px;
            padding: 10px 16px;
            border-radius: 18px;
            border: 1px solid alpha(@axys_fg, {border_alpha});
            background: alpha(@axys_fg, 0.055);
            box-shadow: none;
            font-size: 0.92em;
            color: @axys_fg;
        }}

        .search-main:focus {{
            border-color: alpha(@axys_accent, 0.55);
            background: alpha(@axys_fg, 0.08);
        }}

        .tab {{
            min-height: 30px;
            padding: 2px 4px 2px 9px;
            border-radius: 9px;
            background: transparent;
        }}

        .tab:hover {{
            background: alpha(@axys_fg, 0.06);
        }}

        .tab.active {{
            background: alpha(@axys_fg, 0.10);
        }}

        .tab-select {{
            padding: 2px 5px;
            border-radius: 7px;
            background: transparent;
            box-shadow: none;
            color: @axys_fg;
        }}

        .tab-close {{
            opacity: 0;
            min-width: 20px;
            min-height: 20px;
            padding: 0;
            border-radius: 6px;
        }}

        .tab:hover .tab-close,
        .tab.active .tab-close {{
            opacity: 1;
        }}

        .newtab-logo {{
            font-size: 3.8em;
            font-weight: 600;
            padding: 4px 18px;
            border-radius: 12px;
            background: transparent;
            box-shadow: none;
            color: @axys_fg;
        }}

        .newtab-logo:hover {{
            background: alpha(@axys_fg, 0.06);
        }}

        .newtab-logo:active {{
            background: alpha(@axys_fg, 0.10);
        }}

        button.flat {{
            min-width: 30px;
            min-height: 30px;
            padding: 4px;
            border-radius: 9px;
            box-shadow: none;
        }}

        button.flat:hover {{
            background: alpha(@axys_fg, 0.07);
        }}

        button.flat:active {{
            background: alpha(@axys_fg, 0.12);
        }}

        .private-banner {{
            padding: 6px 12px;
            border-radius: 12px;
            background: alpha(@axys_accent, 0.10);
            color: @axys_fg;
        }}

        .bookmark-bar {{
            padding: 4px 8px;
            margin: 0;
            min-height: 30px;
            border-spacing: 4px;
        }}

        .bookmark-button {{
            min-height: 26px;
            min-width: 0;
            padding: 4px 10px;
            margin: 2px 3px;
            border-radius: 9px;
            background-color: alpha(@axys_fg, 0.045);
            color: @axys_fg;
            border: 1px solid alpha(@axys_fg, {subtle_border_alpha});
        }}

        .bookmark-button:hover {{
            background-color: alpha(@axys_fg, 0.08);
            border-radius: 9px;
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
    @define-color axys_bg #242424;
    @define-color axys_fg #e9ebec;
    @define-color axys_accent #4c8bf5;
"#;

const LIGHT_PALETTE: &str = r#"
    @define-color axys_bg #e6e6e6;
    @define-color axys_fg #000000;
    @define-color axys_accent #2f6fe4;
"#;
