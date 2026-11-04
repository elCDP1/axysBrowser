//! Window controls drawn by the browser itself.
//!
//! WSLg (and some Linux window managers) draw their own titlebar outside the
//! GTK content, where application CSS can never reach it. The main window is
//! therefore undecorated and these buttons replace the host's controls, so
//! they follow the light/dark browser theme like the rest of the chrome.

use gtk::prelude::*;
use gtk::{ApplicationWindow, Box, Button, Orientation};

fn maximize_icon(maximized: bool) -> &'static str {
    if maximized {
        "window-restore-symbolic"
    } else {
        "window-maximize-symbolic"
    }
}

fn maximize_tooltip(maximized: bool) -> String {
    if maximized {
        rust_i18n::t!("app.restore_window").to_string()
    } else {
        rust_i18n::t!("app.maximize_window").to_string()
    }
}

/// Builds the minimize / maximize / close cluster. The caller parents it in
/// the tab strip; every callback holds the window weakly so closing the
/// window is not blocked by its own chrome.
pub fn build(window: &ApplicationWindow) -> Box {
    let controls = Box::new(Orientation::Horizontal, 2);

    controls.add_css_class("window-controls");
    controls.set_valign(gtk::Align::Center);

    let minimize = Button::builder()
        .icon_name("window-minimize-symbolic")
        .tooltip_text(rust_i18n::t!("app.minimize_window").as_ref())
        .build();

    minimize.add_css_class("flat");
    minimize.add_css_class("window-control");

    {
        let window = window.downgrade();

        minimize.connect_clicked(move |_| {
            if let Some(window) = window.upgrade() {
                window.minimize();
            }
        });
    }

    let maximize = Button::builder()
        .icon_name(maximize_icon(window.is_maximized()))
        .tooltip_text(maximize_tooltip(window.is_maximized()).as_str())
        .build();

    maximize.add_css_class("flat");
    maximize.add_css_class("window-control");

    {
        let window = window.downgrade();

        maximize.connect_clicked(move |_| {
            if let Some(window) = window.upgrade() {
                if window.is_maximized() {
                    window.unmaximize();
                } else {
                    window.maximize();
                }
            }
        });
    }

    {
        let maximize = maximize.clone();

        window.connect_maximized_notify(move |window| {
            maximize.set_icon_name(maximize_icon(window.is_maximized()));
            maximize.set_tooltip_text(Some(&maximize_tooltip(window.is_maximized())));
        });
    }

    let close = Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text(rust_i18n::t!("app.close_window").as_ref())
        .build();

    close.add_css_class("flat");
    close.add_css_class("window-control");
    close.add_css_class("window-close");

    {
        let window = window.downgrade();

        close.connect_clicked(move |_| {
            if let Some(window) = window.upgrade() {
                window.close();
            }
        });
    }

    controls.append(&minimize);
    controls.append(&maximize);
    controls.append(&close);

    controls
}

#[cfg(test)]
mod tests {
    use super::maximize_icon;

    #[test]
    fn the_maximize_button_advertises_the_opposite_action() {
        assert_eq!(maximize_icon(false), "window-maximize-symbolic");
        assert_eq!(maximize_icon(true), "window-restore-symbolic");
    }
}
