use gtk::prelude::*;
use gtk::{Box, Button, GestureClick, Orientation, PropagationPhase};
use std::cell::RefCell;
use std::rc::Rc;

use crate::browser::tabs::Tab;

use super::logo::build_logo_button;

/// What the tab row currently shows. `refresh` is called on navigation,
/// tab switches, title changes and language changes; rebuilding every
/// button when the result would be identical cancels in-flight clicks and
/// costs a full widget tree, so it is skipped.
#[derive(PartialEq, Eq)]
struct Rendered {
    rows: Vec<(usize, String)>,
    active: usize,
    close_tooltip: String,
}

pub struct TabBar {
    pub root: Box,
    tabs_box: Box,
    logo_button: Button,
    new_button: Button,
    on_select: Rc<dyn Fn(usize)>,
    on_close: Rc<dyn Fn(usize)>,
    rendered: RefCell<Option<Rendered>>,
}

impl TabBar {
    pub fn new(
        on_new: Rc<dyn Fn()>,
        on_select: Rc<dyn Fn(usize)>,
        on_close: Rc<dyn Fn(usize)>,
        on_about: Rc<dyn Fn()>,
    ) -> Self {
        let root = Box::new(Orientation::Horizontal, 5);

        root.set_margin_top(5);
        root.set_margin_bottom(3);
        root.set_margin_start(8);
        root.set_margin_end(8);

        let logo_space = Box::new(Orientation::Horizontal, 0);

        logo_space.set_width_request(80);

        logo_space.set_height_request(40);

        logo_space.set_hexpand(false);

        logo_space.set_vexpand(false);

        let logo_button = build_logo_button(on_about);

        logo_space.append(&logo_button);

        let tabs_box = Box::new(Orientation::Horizontal, 5);

        tabs_box.set_hexpand(true);

        let new_button = Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text(rust_i18n::t!("app.new_tab").as_ref())
            .build();

        new_button.add_css_class("flat");

        new_button.connect_clicked(move |_| {
            on_new();
        });

        root.append(&logo_space);

        root.append(&tabs_box);

        root.append(&new_button);

        Self {
            root,
            tabs_box,
            logo_button,
            new_button,
            on_select,
            on_close,
            rendered: RefCell::new(None),
        }
    }

    /// Re-translates the static tooltips (language change; tab titles
    /// refresh through [`TabBar::refresh`] already).
    pub fn refresh_labels(&self) {
        self.new_button
            .set_tooltip_text(Some(&rust_i18n::t!("app.new_tab")));
        self.logo_button
            .set_tooltip_text(Some(&rust_i18n::t!("about.tooltip")));
    }

    pub fn refresh(&self, tabs: &[Tab], active_id: usize) {
        let close_tooltip = rust_i18n::t!("app.close_tab").to_string();
        let fallback_title = rust_i18n::t!("app.new_tab").to_string();

        let rendered = Rendered {
            rows: tabs
                .iter()
                .map(|tab| {
                    let title = if tab.title.is_empty() {
                        fallback_title.clone()
                    } else {
                        tab.title.clone()
                    };

                    (tab.id, title)
                })
                .collect(),
            active: active_id,
            close_tooltip: close_tooltip.clone(),
        };

        {
            let mut last = self.rendered.borrow_mut();

            if last.as_ref() == Some(&rendered) {
                return;
            }

            *last = Some(rendered);
        }

        while let Some(child) = self.tabs_box.first_child() {
            self.tabs_box.remove(&child);
        }

        for tab_data in tabs {
            let tab = Box::new(Orientation::Horizontal, 2);

            tab.set_hexpand(true);

            tab.set_halign(gtk::Align::Fill);

            tab.add_css_class("tab");

            if tab_data.id == active_id {
                tab.add_css_class("active");
            }

            let title = if tab_data.title.is_empty() {
                fallback_title.as_str()
            } else {
                &tab_data.title
            };

            let select = Button::with_label(title);

            // Long titles must shrink instead of widening the whole window.
            if let Some(label) = select.child().and_downcast::<gtk::Label>() {
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_max_width_chars(24);
            }

            select.set_hexpand(true);

            select.set_halign(gtk::Align::Fill);

            select.add_css_class("tab-select");

            let close = Button::builder()
                .icon_name("window-close-symbolic")
                .tooltip_text(close_tooltip.as_str())
                .build();

            // Fixed small box, vertically centered in the tab row, so the
            // icon cannot drift when the row height changes.
            close.set_valign(gtk::Align::Center);

            close.add_css_class("tab-close");

            close.add_css_class("flat");

            let id = tab_data.id;

            let on_select = self.on_select.clone();

            select.connect_clicked(move |_| {
                on_select(id);
            });

            let id = tab_data.id;

            let on_close = self.on_close.clone();

            close.connect_clicked(move |_| {
                on_close(id);
            });

            let id = tab_data.id;

            let on_close = self.on_close.clone();

            let middle_click = GestureClick::new();

            middle_click.set_button(2);

            middle_click.set_propagation_phase(PropagationPhase::Capture);

            middle_click.connect_pressed(move |_, _, _, _| {
                on_close(id);
            });

            tab.add_controller(middle_click);

            tab.append(&select);

            tab.append(&close);

            self.tabs_box.append(&tab);
        }
    }
}
