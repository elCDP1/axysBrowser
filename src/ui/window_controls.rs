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

/// Makes Escape close a dialog window. A plain `gtk::Window` ignores Escape
/// (only `GtkDialog` handles it), so the bookmark and permission prompts could
/// only be dismissed with the mouse.
pub fn close_on_escape(window: &gtk::Window) {
    let weak = window.downgrade();

    let key = gtk::EventControllerKey::new();

    key.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::Escape
            && let Some(window) = weak.upgrade()
        {
            window.close();

            return gtk::glib::Propagation::Stop;
        }

        gtk::glib::Propagation::Proceed
    });

    window.add_controller(key);
}

/// Thickness (px) of the invisible resize strips along the window edges, and
/// the side of the square grips in the corners.
const GRIP_EDGE: i32 = 5;
const GRIP_CORNER: i32 = 10;

/// One resize grip: where it sits, which edge it drags and its cursor.
struct Grip {
    edge: gtk::gdk::SurfaceEdge,
    cursor: &'static str,
    halign: gtk::Align,
    valign: gtk::Align,
}

fn grips() -> [Grip; 8] {
    use gtk::Align::{End, Fill, Start};
    use gtk::gdk::SurfaceEdge as Edge;

    // Edges first, corners last: later overlay children sit on top, so a
    // corner wins where it overlaps the ends of two edge strips.
    [
        Grip {
            edge: Edge::North,
            cursor: "n-resize",
            halign: Fill,
            valign: Start,
        },
        Grip {
            edge: Edge::South,
            cursor: "s-resize",
            halign: Fill,
            valign: End,
        },
        Grip {
            edge: Edge::West,
            cursor: "w-resize",
            halign: Start,
            valign: Fill,
        },
        Grip {
            edge: Edge::East,
            cursor: "e-resize",
            halign: End,
            valign: Fill,
        },
        Grip {
            edge: Edge::NorthWest,
            cursor: "nw-resize",
            halign: Start,
            valign: Start,
        },
        Grip {
            edge: Edge::NorthEast,
            cursor: "ne-resize",
            halign: End,
            valign: Start,
        },
        Grip {
            edge: Edge::SouthWest,
            cursor: "sw-resize",
            halign: Start,
            valign: End,
        },
        Grip {
            edge: Edge::SouthEast,
            cursor: "se-resize",
            halign: End,
            valign: End,
        },
    ]
}

/// Adds invisible resize strips around the window.
///
/// The window is undecorated, and GTK4 draws no resize border for an
/// undecorated window (there is no client-side shadow to grab), so without
/// these the window could not be resized with the mouse at all outside
/// compositors that add their own grips. Each strip starts a compositor
/// driven resize; they hide while the window is maximized or fullscreen.
pub fn add_resize_grips(overlay: &gtk::Overlay, window: &ApplicationWindow) {
    let mut widgets = Vec::new();

    for grip in grips() {
        let corner = matches!(
            grip.edge,
            gtk::gdk::SurfaceEdge::NorthWest
                | gtk::gdk::SurfaceEdge::NorthEast
                | gtk::gdk::SurfaceEdge::SouthWest
                | gtk::gdk::SurfaceEdge::SouthEast
        );

        let strip = Box::new(Orientation::Horizontal, 0);

        strip.set_halign(grip.halign);
        strip.set_valign(grip.valign);
        strip.set_cursor_from_name(Some(grip.cursor));

        let (width, height) = if corner {
            (GRIP_CORNER, GRIP_CORNER)
        } else if grip.halign == gtk::Align::Fill {
            (-1, GRIP_EDGE)
        } else {
            (GRIP_EDGE, -1)
        };

        strip.set_size_request(width, height);

        let click = gtk::GestureClick::builder()
            .button(gtk::gdk::BUTTON_PRIMARY)
            .build();

        {
            let window = window.downgrade();
            let edge = grip.edge;

            click.connect_pressed(move |gesture, _presses, x, y| {
                let Some(window) = window.upgrade() else {
                    return;
                };

                if window.is_maximized() || window.is_fullscreen() {
                    return;
                }

                let (Some(widget), Some(device), Some(surface)) =
                    (gesture.widget(), gesture.device(), window.surface())
                else {
                    return;
                };

                let Ok(toplevel) = surface.downcast::<gtk::gdk::Toplevel>() else {
                    return;
                };

                let (surface_x, surface_y) = widget
                    .translate_coordinates(&window, x, y)
                    .unwrap_or((x, y));

                gesture.set_state(gtk::EventSequenceState::Claimed);

                toplevel.begin_resize(
                    edge,
                    Some(&device),
                    gtk::gdk::BUTTON_PRIMARY as i32,
                    surface_x,
                    surface_y,
                    gesture.current_event_time(),
                );
            });
        }

        strip.add_controller(click);

        overlay.add_overlay(&strip);

        widgets.push(strip);
    }

    let sync = move |window: &ApplicationWindow| {
        let resizable = !window.is_maximized() && !window.is_fullscreen();

        for strip in &widgets {
            strip.set_visible(resizable);
        }
    };

    sync(window);

    let sync = std::rc::Rc::new(sync);

    {
        let sync = sync.clone();

        window.connect_maximized_notify(move |window| sync(window));
    }

    window.connect_fullscreened_notify(move |window| sync(window));
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
