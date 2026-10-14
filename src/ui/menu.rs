use gtk::prelude::*;
use gtk::{MenuButton, gio};
use std::cell::Cell;
use std::rc::Rc;

pub fn build_menu_model(private_mode: bool) -> gio::Menu {
    let navigation = gio::Menu::new();

    navigation.append(Some(&rust_i18n::t!("app.new_tab")), Some("win.new-tab"));

    navigation.append(
        Some(&rust_i18n::t!("app.new_window")),
        Some("win.new-window"),
    );

    navigation.append(
        Some(&rust_i18n::t!("app.reopen_tab")),
        Some("win.reopen-tab"),
    );

    let menu = gio::Menu::new();

    menu.append_submenu(Some(&rust_i18n::t!("menu.navigation")), &navigation);

    menu.append(Some(&rust_i18n::t!("history.title")), Some("win.history"));

    menu.append(Some(&rust_i18n::t!("app.downloads")), Some("win.downloads"));

    // Toggle: inside a private window this item exits back to normal mode.
    if private_mode {
        menu.append(Some(&rust_i18n::t!("privacy.exit")), Some("win.privacy"));
    } else {
        menu.append(
            Some(&rust_i18n::t!("app.privacy_window")),
            Some("win.privacy"),
        );
    }

    menu.append(Some(&rust_i18n::t!("app.tools")), Some("win.tools"));

    menu.append(Some(&rust_i18n::t!("app.settings")), Some("win.settings"));

    menu
}

pub fn build_menu(private_mode: bool) -> MenuButton {
    let button = MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(rust_i18n::t!("app.menu").as_ref())
        .menu_model(&build_menu_model(private_mode))
        .build();

    track_menu_button(&button);

    button
}

/// Insurance against stuck toggle state: if a popover is dismissed without
/// the button resetting (observed on some Wayland/WSLg links, where the
/// menu then refuses to reopen), force it back to inactive on close.
fn track_menu_button(button: &MenuButton) {
    let hooked = Rc::new(Cell::new(false));

    // Note: MenuButton is not a Button subclass, so there is no `clicked`
    // signal — active-notify covers both open and close transitions.
    button.connect_active_notify(move |button| {
        if hooked.get() {
            return;
        }

        if let Some(popover) = button.popover() {
            let back = button.clone();

            popover.connect_closed(move |_| {
                back.set_active(false);
            });

            hooked.set(true);
        }
    });
}
