use gtk::{MenuButton, gio};

pub fn build_menu_model() -> gio::Menu {
    let navigation = gio::Menu::new();

    navigation.append(Some(&rust_i18n::t!("app.new_tab")), Some("win.new-tab"));

    navigation.append(
        Some(&rust_i18n::t!("app.new_window")),
        Some("win.new-window"),
    );

    let menu = gio::Menu::new();

    menu.append_submenu(Some(&rust_i18n::t!("menu.navigation")), &navigation);

    menu.append(Some(&rust_i18n::t!("history.title")), Some("win.history"));

    menu.append(Some(&rust_i18n::t!("app.downloads")), Some("win.downloads"));

    menu.append(
        Some(&rust_i18n::t!("app.privacy_window")),
        Some("win.privacy"),
    );

    menu.append(Some(&rust_i18n::t!("app.tools")), Some("win.tools"));

    menu.append(Some(&rust_i18n::t!("app.settings")), Some("win.settings"));

    menu
}

pub fn build_menu() -> MenuButton {
    MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(rust_i18n::t!("app.menu").as_ref())
        .menu_model(&build_menu_model())
        .build()
}
