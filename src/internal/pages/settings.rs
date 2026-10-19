use gtk::prelude::*;
use gtk::{Box, Label, Orientation, ScrolledWindow, Switch};
use std::rc::Rc;

use crate::app_state::AppState;
use crate::ui::menu::build_inline_picker;

fn row(title: &str, description: &str) -> Box {
    row_with_description(title, description).0
}

/// Like [`row`], but also returns the description label so callers can
/// refresh its text in place (e.g. the remembered-permission count).
fn row_with_description(title: &str, description: &str) -> (Box, Label) {
    let row = Box::new(Orientation::Horizontal, 12);

    let labels = Box::new(Orientation::Vertical, 2);

    labels.set_hexpand(true);

    let title_label = Label::new(Some(title));

    title_label.set_halign(gtk::Align::Start);

    let description_label = Label::new(Some(description));

    description_label.add_css_class("dim-label");

    description_label.set_halign(gtk::Align::Start);

    description_label.set_wrap(true);

    labels.append(&title_label);

    labels.append(&description_label);

    row.append(&labels);

    (row, description_label)
}

pub fn build_settings(
    state: Rc<AppState>,
    on_clear_browsing_data: crate::app_state::ClearBrowsingData,
) -> ScrolledWindow {
    let page = Box::new(Orientation::Vertical, 18);

    page.set_margin_top(28);
    page.set_margin_bottom(28);
    page.set_margin_start(28);
    page.set_margin_end(28);

    page.set_vexpand(true);

    page.set_hexpand(true);

    let title = Label::new(Some(&rust_i18n::t!("settings.title")));

    title.add_css_class("title-1");

    title.set_halign(gtk::Align::Start);

    let appearance_title = Label::new(Some(&rust_i18n::t!("settings.appearance")));

    appearance_title.add_css_class("title-3");

    appearance_title.set_halign(gtk::Align::Start);

    let dark = state.settings.borrow().dark_mode;

    let (appearance, appearance_options) = build_inline_picker(
        vec![
            rust_i18n::t!("settings.dark").to_string(),
            rust_i18n::t!("settings.light").to_string(),
        ],
        if dark { 0 } else { 1 },
        {
            let state = state.clone();

            Rc::new(move |selected| {
                state.set_dark_mode(selected == 0);
            })
        },
    );

    appearance.set_halign(gtk::Align::Start);

    let search_title = Label::new(Some(&rust_i18n::t!("settings.search")));

    search_title.add_css_class("title-3");

    search_title.set_halign(gtk::Align::Start);

    let engine_names: Vec<String> = crate::browser::address::KNOWN_ENGINES
        .iter()
        .map(|(_, name)| name.to_string())
        .collect();

    let selected = crate::browser::address::KNOWN_ENGINES
        .iter()
        .position(|(id, _)| *id == state.settings.borrow().search_engine)
        .unwrap_or(0);

    let (search, search_options) = build_inline_picker(
        engine_names,
        selected,
        {
            let state = state.clone();

            Rc::new(move |picked| {
                if let Some((id, _)) = crate::browser::address::KNOWN_ENGINES.get(picked) {
                    state.set_search_engine(id);
                }
            })
        },
    );

    search.set_halign(gtk::Align::Start);

    let search_note = Label::new(Some(&rust_i18n::t!("settings.search_note")));

    search_note.add_css_class("dim-label");

    search_note.set_halign(gtk::Align::Start);

    let privacy_title = Label::new(Some(&rust_i18n::t!("settings.privacy_security")));

    privacy_title.add_css_class("title-3");

    privacy_title.set_halign(gtk::Align::Start);

    let tracking_row = row(
        &rust_i18n::t!("settings.tracking_prevention"),
        &rust_i18n::t!("settings.tracking_description"),
    );

    let tracking_switch = Switch::new();

    tracking_switch.set_active(state.settings.borrow().tracking_prevention);

    // Fixed height: in tall rows the switch would otherwise stretch.
    tracking_switch.set_valign(gtk::Align::Center);

    tracking_row.append(&tracking_switch);

    {
        let state = state.clone();

        tracking_switch.connect_active_notify(move |switch| {
            state.set_tracking_prevention(switch.is_active());
        });
    }

    let cookies_row = row(
        &rust_i18n::t!("settings.cookies"),
        &rust_i18n::t!("settings.cookies_description"),
    );

    let selected_cookie = match state.settings.borrow().cookie_policy.as_str() {
        "always" => 1,
        "never" => 2,
        _ => 0,
    };

    let (cookies, cookies_options) = build_inline_picker(
        vec![
            rust_i18n::t!("settings.cookies_no_third_party").to_string(),
            rust_i18n::t!("settings.cookies_always").to_string(),
            rust_i18n::t!("settings.cookies_never").to_string(),
        ],
        selected_cookie,
        {
            let state = state.clone();

            Rc::new(move |picked| {
                let policy = match picked {
                    1 => "always",
                    2 => "never",
                    _ => "no-third-party",
                };

                state.set_cookie_policy(policy);
            })
        },
    );

    cookies_row.append(&cookies);

    let clear_row = row(
        &rust_i18n::t!("settings.clear_data"),
        &rust_i18n::t!("settings.clear_data_description"),
    );

    let clear_box = Box::new(Orientation::Vertical, 4);

    clear_box.set_halign(gtk::Align::End);

    let clear_button = gtk::Button::with_label(&rust_i18n::t!("settings.clear_data"));

    clear_button.add_css_class("destructive-action");

    let clear_status = Label::new(None);

    clear_status.add_css_class("dim-label");

    clear_status.set_halign(gtk::Align::End);

    {
        let on_clear_browsing_data = on_clear_browsing_data.clone();
        let clear_status = clear_status.clone();

        clear_button.connect_clicked(move |_| {
            clear_status.set_text(&rust_i18n::t!("settings.clearing"));

            let clear_status = clear_status.clone();

            on_clear_browsing_data(Rc::new(move |ok| {
                clear_status.set_text(&rust_i18n::t!(if ok {
                    "settings.clear_data_done"
                } else {
                    "settings.clear_data_failed"
                }));
            }));
        });
    }

    clear_box.append(&clear_button);
    clear_box.append(&clear_status);
    clear_row.append(&clear_box);

    let developer_row = row(
        &rust_i18n::t!("settings.developer_tools"),
        &rust_i18n::t!("settings.developer_description"),
    );

    let developer_switch = Switch::new();

    developer_switch.set_active(state.settings.borrow().developer_tools);

    developer_switch.set_valign(gtk::Align::Center);

    developer_row.append(&developer_switch);

    {
        let state = state.clone();

        developer_switch.connect_active_notify(move |switch| {
            state.set_developer_tools(switch.is_active());
        });
    }

    let hw_row = row(
        &rust_i18n::t!("settings.hw_accel"),
        &rust_i18n::t!("settings.hw_accel_description"),
    );

    let hw_switch = Switch::new();

    hw_switch.set_active(state.settings.borrow().hardware_acceleration);

    hw_switch.set_valign(gtk::Align::Center);

    hw_row.append(&hw_switch);

    {
        let state = state.clone();

        hw_switch.connect_active_notify(move |switch| {
            state.set_hardware_acceleration(switch.is_active());
        });
    }

    let permissions_desc = format!(
        "{} ({})",
        rust_i18n::t!("settings.permissions_description"),
        state.permissions.decision_count()
    );

    let (permissions_row, permissions_desc_label) =
        row_with_description(&rust_i18n::t!("settings.permissions"), &permissions_desc);

    let permissions_reset = gtk::Button::with_label(&rust_i18n::t!("settings.permissions_reset"));

    permissions_reset.set_valign(gtk::Align::Center);

    let permissions_status = Label::new(None);

    permissions_status.add_css_class("dim-label");

    {
        let state = state.clone();
        let permissions_status = permissions_status.clone();
        let permissions_desc_label = permissions_desc_label.clone();

        permissions_reset.connect_clicked(move |_| {
            state.reset_permissions();

            // Refresh the count in place: the description is built once.
            permissions_desc_label.set_text(&format!(
                "{} ({})",
                rust_i18n::t!("settings.permissions_description"),
                state.permissions.decision_count()
            ));

            permissions_status.set_text(&rust_i18n::t!("settings.permissions_cleared"));
        });
    }

    permissions_row.append(&permissions_status);
    permissions_row.append(&permissions_reset);

    let language_title = Label::new(Some(&rust_i18n::t!("settings.language")));

    language_title.add_css_class("title-3");

    language_title.set_halign(gtk::Align::Start);

    let selected_language = match state.settings.borrow().language.as_str() {
        "es" => 1,
        _ => 0,
    };

    let (language, language_options) = build_inline_picker(
        vec![
            rust_i18n::t!("settings.english").to_string(),
            rust_i18n::t!("settings.spanish").to_string(),
        ],
        selected_language,
        {
            let state = state.clone();

            Rc::new(move |picked| {
                state.set_language(if picked == 1 { "es" } else { "en" });
            })
        },
    );

    language.set_halign(gtk::Align::Start);

    page.append(&title);

    page.append(&appearance_title);

    page.append(&appearance);
    page.append(&appearance_options);

    page.append(&search_title);

    page.append(&search);
    page.append(&search_options);

    page.append(&search_note);

    page.append(&privacy_title);

    page.append(&tracking_row);

    page.append(&cookies_row);
    // The toggle lives at the row's right end, so the list opens aligned
    // to the right under it instead of at the page's left edge.
    cookies_options.set_halign(gtk::Align::End);
    page.append(&cookies_options);

    page.append(&clear_row);

    page.append(&developer_row);

    page.append(&hw_row);

    page.append(&permissions_row);

    page.append(&language_title);

    page.append(&language);
    page.append(&language_options);
    let scroller = ScrolledWindow::new();

    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&page));

    scroller
}
