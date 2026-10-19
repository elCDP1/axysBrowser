use gtk::prelude::*;
use gtk::{Box, Button, Label, Orientation, Separator};
use std::rc::Rc;

use crate::app_state::AppState;
use crate::browser::address::KNOWN_ENGINES;
use crate::ui::menu::build_inline_picker;

pub fn build_welcome(state: Rc<AppState>, on_continue: Rc<dyn Fn(String)>) -> Box {
    let page = Box::new(Orientation::Vertical, 18);

    page.set_vexpand(true);
    page.set_hexpand(true);
    page.set_valign(gtk::Align::Center);
    page.set_halign(gtk::Align::Center);

    let title = Label::new(Some(&rust_i18n::t!("welcome.heading")));
    title.add_css_class("title-1");

    let subtitle = Label::new(Some(&rust_i18n::t!("welcome.subtitle")));

    subtitle.add_css_class("dim-label");

    let section = Box::new(Orientation::Vertical, 8);

    let search_label = Label::new(Some(&rust_i18n::t!("welcome.search")));
    search_label.set_halign(gtk::Align::Start);

    let names: Vec<String> = KNOWN_ENGINES
        .iter()
        .map(|(_, name)| name.to_string())
        .collect();

    let current = state.settings.borrow().search_engine.clone();

    let selected = KNOWN_ENGINES
        .iter()
        .position(|(id, _)| *id == current)
        .unwrap_or(0);

    let (search, search_options) = build_inline_picker(names, selected, {
        let state = state.clone();

        Rc::new(move |picked| {
            if let Some((id, _)) = KNOWN_ENGINES.get(picked) {
                state.set_search_engine(id);
            }
        })
    });

    let notice = Label::new(Some(&rust_i18n::t!("welcome.search_note")));

    notice.add_css_class("dim-label");
    notice.set_wrap(true);
    notice.set_max_width_chars(52);

    section.append(&search_label);
    section.append(&search);
    // Centered column: keep toggle and list centered together.
    search.set_halign(gtk::Align::Center);
    search_options.set_halign(gtk::Align::Center);
    section.append(&search_options);
    section.append(&notice);

    let separator = Separator::new(Orientation::Horizontal);

    let description = Label::new(Some(&rust_i18n::t!("welcome.description")));

    description.set_wrap(true);
    description.set_max_width_chars(54);
    description.set_justify(gtk::Justification::Center);

    let continue_button = Button::with_label(&rust_i18n::t!("welcome.continue"));
    continue_button.add_css_class("suggested-action");
    continue_button.set_halign(gtk::Align::Center);

    continue_button.connect_clicked(move |_| {
        on_continue("axys://newtab".to_string());
    });

    page.append(&title);
    page.append(&subtitle);
    page.append(&section);
    page.append(&separator);
    page.append(&description);
    page.append(&continue_button);

    page
}
