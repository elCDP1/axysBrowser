use gtk::prelude::*;
use gtk::{Box, DropDown, Label, Orientation, Switch};
use std::rc::Rc;

use crate::app_state::AppState;

fn row(title: &str, description: &str) -> Box {
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

    row
}

pub fn build_settings(state: Rc<AppState>, on_extensions_changed: Rc<dyn Fn(bool)>) -> Box {
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

    let appearance = DropDown::from_strings(&[
        &rust_i18n::t!("settings.dark"),
        &rust_i18n::t!("settings.light"),
    ]);

    let dark = state.settings.borrow().dark_mode;

    appearance.set_selected(if dark { 0 } else { 1 });

    appearance.set_halign(gtk::Align::Start);

    {
        let state = state.clone();

        appearance.connect_selected_notify(move |dropdown| {
            state.set_dark_mode(dropdown.selected() == 0);
        });
    }

    let extensions_row = row(
        &rust_i18n::t!("settings.extensions"),
        &rust_i18n::t!("settings.extensions_description"),
    );

    let extensions_switch = Switch::new();

    extensions_switch.set_active(state.settings.borrow().show_extensions);

    extensions_row.append(&extensions_switch);

    {
        let state = state.clone();

        let callback = on_extensions_changed.clone();

        extensions_switch.connect_active_notify(move |switch| {
            let visible = switch.is_active();

            state.set_extensions_visible(visible);

            callback(visible);
        });
    }

    let search_title = Label::new(Some(&rust_i18n::t!("settings.search")));

    search_title.add_css_class("title-3");

    search_title.set_halign(gtk::Align::Start);

    let search = DropDown::from_strings(&["Brave Search", "DuckDuckGo", "Google", "Bing"]);

    let selected = match state.settings.borrow().search_engine.as_str() {
        "duckduckgo" => 1,
        "google" => 2,
        "bing" => 3,
        _ => 0,
    };

    search.set_selected(selected);

    search.set_halign(gtk::Align::Start);

    {
        let state = state.clone();

        search.connect_selected_notify(move |dropdown| {
            let engine = match dropdown.selected() {
                1 => "duckduckgo",
                2 => "google",
                3 => "bing",
                _ => "brave",
            };

            state.set_search_engine(engine);
        });
    }

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

    tracking_row.append(&tracking_switch);

    {
        let state = state.clone();

        tracking_switch.connect_active_notify(move |switch| {
            state.set_tracking_prevention(switch.is_active());
        });
    }

    let developer_row = row(
        &rust_i18n::t!("settings.developer_tools"),
        &rust_i18n::t!("settings.developer_description"),
    );

    let developer_switch = Switch::new();

    developer_switch.set_active(state.settings.borrow().developer_tools);

    developer_row.append(&developer_switch);

    {
        let state = state.clone();

        developer_switch.connect_active_notify(move |switch| {
            state.set_developer_tools(switch.is_active());
        });
    }

    let language_title = Label::new(Some(&rust_i18n::t!("settings.language")));

    language_title.add_css_class("title-3");

    language_title.set_halign(gtk::Align::Start);

    let english_label = rust_i18n::t!("settings.english");

    let spanish_label = rust_i18n::t!("settings.spanish");

    let language = DropDown::from_strings(&[english_label.as_ref(), spanish_label.as_ref()]);

    let selected_language = match state.settings.borrow().language.as_str() {
        "es" => 1,
        _ => 0,
    };

    language.set_selected(selected_language);

    language.set_halign(gtk::Align::Start);

    {
        let state = state.clone();

        language.connect_selected_notify(move |dropdown| {
            let locale = match dropdown.selected() {
                1 => "es",
                _ => "en",
            };

            state.set_language(locale);
        });
    }

    page.append(&title);

    page.append(&appearance_title);

    page.append(&appearance);

    page.append(&extensions_row);

    page.append(&search_title);

    page.append(&search);

    page.append(&search_note);

    page.append(&privacy_title);

    page.append(&tracking_row);

    page.append(&developer_row);

    page.append(&language_title);

    page.append(&language);

    page
}
