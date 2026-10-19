//! Keyboard shortcuts (Chromium parity).
//!
//! Extracted from `ui::window`: the whole key handler lives here and
//! receives everything it needs through [`ShortcutContext`], so the main
//! window file stays focused on wiring.

use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, EventControllerKey, gdk, gio, glib::Propagation};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use webkit6::WebView;
use webkit6::prelude::WebViewExt;

use crate::app_state::AppState;
use crate::browser::engine::BrowserEngine;
use crate::browser::tabs::{NewTabCallback, Tab};

use super::bookmarks::BookmarkBar;
use super::findbar::FindBar;
use super::toolbar::Toolbar;
use super::window::{build_private_window, build_window};

/// Everything the shortcut handler needs, as plain shared state.
pub struct ShortcutContext {
    pub new_tab: NewTabCallback,
    pub close_tab: Rc<dyn Fn(usize)>,
    pub reopen_closed_tab: Rc<dyn Fn()>,
    pub select_tab: Rc<dyn Fn(usize)>,
    pub tabs: Rc<RefCell<Vec<Tab>>>,
    pub active_id: Rc<Cell<usize>>,
    pub toolbar_slot: Rc<RefCell<Option<Toolbar>>>,
    pub current_web_view: Rc<RefCell<Option<WebView>>>,
    pub go_back: Rc<dyn Fn()>,
    pub go_forward: Rc<dyn Fn()>,
    pub navigate_tab: Rc<dyn Fn(usize, String)>,
    pub app: Application,
    pub state: Rc<AppState>,
    pub private_mode: bool,
    pub window_weak: gtk::glib::WeakRef<ApplicationWindow>,
    pub findbar_slot: Rc<RefCell<Option<FindBar>>>,
    pub bookmark_bar_slot: Rc<RefCell<Option<BookmarkBar>>>,
}

/// Installs the key-pressed controller on `window`.
pub fn install_keyboard_shortcuts(window: &ApplicationWindow, ctx: ShortcutContext) {
    let ShortcutContext {
        new_tab,
        close_tab,
        reopen_closed_tab,
        select_tab,
        tabs,
        active_id,
        toolbar_slot,
        current_web_view,
        go_back,
        go_forward,
        navigate_tab,
        app,
        state,
        private_mode,
        window_weak,
        findbar_slot,
        bookmark_bar_slot,
    } = ctx;

    let keyboard = EventControllerKey::new();

    {
        let new_tab = new_tab.clone();
        let close_tab = close_tab.clone();
        let reopen_closed_tab = reopen_closed_tab.clone();
        let select_tab = select_tab.clone();
        let tabs = tabs.clone();
        let active_id = active_id.clone();
        let toolbar_slot = toolbar_slot.clone();
        let current_web_view = current_web_view.clone();
        let go_back = go_back.clone();
        let go_forward = go_forward.clone();
        let navigate_tab = navigate_tab.clone();
        let app_for_shortcuts = app.clone();
        let state_for_shortcuts = state.clone();
        let window_weak = window_weak.clone();
        let findbar_slot = findbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();

        keyboard.connect_key_pressed(move |_, key, _, modifiers| {
            let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);

            let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);

            let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);

            if ctrl {
                if let Some(ch) = key.to_unicode() {
                    match ch.to_ascii_lowercase() {
                        'n' => {
                            if shift {
                                // Same toggle as the menu item: entering
                                // private mode, or leaving it when active.
                                if private_mode {
                                    build_window(&app_for_shortcuts, state_for_shortcuts.clone());

                                    if let Some(window) = window_weak.upgrade() {
                                        window.close();
                                    }
                                } else {
                                    build_private_window(
                                        &app_for_shortcuts,
                                        state_for_shortcuts.clone(),
                                    );
                                }
                            } else {
                                build_window(&app_for_shortcuts, state_for_shortcuts.clone());
                            }

                            return Propagation::Stop;
                        }

                        't' => {
                            // Chromium parity: Ctrl+Shift+T reopens the last
                            // closed tab instead of opening a blank one.
                            if shift {
                                reopen_closed_tab();
                            } else {
                                new_tab(None);
                            }

                            return Propagation::Stop;
                        }

                        'w' => {
                            close_tab(active_id.get());

                            return Propagation::Stop;
                        }

                        'l' => {
                            // Focus the address without deleting anything;
                            // the focus handler selects everything.
                            if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                                toolbar.address.grab_focus();
                            }

                            return Propagation::Stop;
                        }

                        'r' => {
                            if let Some(view) = current_web_view.borrow().as_ref() {
                                if shift {
                                    view.reload_bypass_cache();
                                } else {
                                    BrowserEngine::reload(view);
                                }
                            }

                            return Propagation::Stop;
                        }

                        '+' | '=' => {
                            if let Some(view) = current_web_view.borrow().as_ref() {
                                view.set_zoom_level((view.zoom_level() + 0.1).clamp(0.25, 5.0));
                            }

                            return Propagation::Stop;
                        }

                        '-' => {
                            if let Some(view) = current_web_view.borrow().as_ref() {
                                view.set_zoom_level((view.zoom_level() - 0.1).clamp(0.25, 5.0));
                            }

                            return Propagation::Stop;
                        }

                        '0' => {
                            if let Some(view) = current_web_view.borrow().as_ref() {
                                view.set_zoom_level(1.0);
                            }

                            return Propagation::Stop;
                        }

                        'j' => {
                            new_tab(Some("axys://downloads"));

                            return Propagation::Stop;
                        }

                        'h' => {
                            new_tab(Some("axys://history"));

                            return Propagation::Stop;
                        }

                        'd' if !shift => {
                            if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                                toolbar.activate_bookmark();
                            }

                            return Propagation::Stop;
                        }

                        // Ctrl+F opens the find-in-page bar.
                        'f' => {
                            if let Some(findbar) = findbar_slot.borrow().as_ref() {
                                findbar.show();
                            }

                            return Propagation::Stop;
                        }

                        // Ctrl+O opens a local file in the active tab.
                        'o' => {
                            if let Some(parent) = app_for_shortcuts.active_window() {
                                let dialog = gtk::FileDialog::new();

                                let navigate_tab = navigate_tab.clone();
                                let active_id = active_id.clone();

                                dialog.open(
                                    Some(&parent),
                                    None::<&gio::Cancellable>,
                                    move |result| {
                                        if let Ok(file) = result {
                                            navigate_tab(active_id.get(), file.uri().to_string());
                                        }
                                    },
                                );
                            }

                            return Propagation::Stop;
                        }

                        _ => {}
                    }
                }

                if key == gdk::Key::Tab {
                    let tabs_ref = tabs.borrow();

                    if tabs_ref.is_empty() {
                        return Propagation::Stop;
                    }

                    let current = tabs_ref
                        .iter()
                        .position(|tab| tab.id == active_id.get())
                        .unwrap_or(0);

                    let next = if shift {
                        if current == 0 {
                            tabs_ref.len() - 1
                        } else {
                            current - 1
                        }
                    } else {
                        (current + 1) % tabs_ref.len()
                    };

                    let next_id = tabs_ref[next].id;

                    drop(tabs_ref);

                    select_tab(next_id);

                    return Propagation::Stop;
                }

                if key == gdk::Key::Page_Up {
                    let tabs_ref = tabs.borrow();

                    if tabs_ref.is_empty() {
                        return Propagation::Stop;
                    }

                    let current = tabs_ref
                        .iter()
                        .position(|tab| tab.id == active_id.get())
                        .unwrap_or(0);

                    let next = if current == 0 {
                        tabs_ref.len() - 1
                    } else {
                        current - 1
                    };

                    let next_id = tabs_ref[next].id;

                    drop(tabs_ref);

                    select_tab(next_id);

                    return Propagation::Stop;
                }

                if key == gdk::Key::Page_Down {
                    let tabs_ref = tabs.borrow();

                    if tabs_ref.is_empty() {
                        return Propagation::Stop;
                    }

                    let current = tabs_ref
                        .iter()
                        .position(|tab| tab.id == active_id.get())
                        .unwrap_or(0);

                    let next = (current + 1) % tabs_ref.len();

                    let next_id = tabs_ref[next].id;

                    drop(tabs_ref);

                    select_tab(next_id);

                    return Propagation::Stop;
                }

                if let Some(ch) = key.to_unicode()
                    && let Some(digit) = ch.to_digit(10)
                    && (1..=9).contains(&digit)
                {
                    let index = digit as usize - 1;

                    let tabs_ref = tabs.borrow();

                    if index < tabs_ref.len() {
                        let id = tabs_ref[index].id;

                        drop(tabs_ref);

                        select_tab(id);
                    }

                    return Propagation::Stop;
                }
            }

            if alt {
                match key {
                    gdk::Key::Left => {
                        go_back();

                        return Propagation::Stop;
                    }

                    gdk::Key::Right => {
                        go_forward();

                        return Propagation::Stop;
                    }

                    _ => {}
                }
            }

            if key == gdk::Key::F5 {
                if let Some(view) = current_web_view.borrow().as_ref() {
                    BrowserEngine::reload(view);
                }

                return Propagation::Stop;
            }

            // F6 focuses the address bar (Chromium parity with Ctrl+L).
            if key == gdk::Key::F6 {
                if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                    toolbar.address.grab_focus();
                }

                return Propagation::Stop;
            }

            if key == gdk::Key::F11 {
                if let Some(win) = window_weak.upgrade() {
                    if win.is_fullscreen() {
                        win.unfullscreen();
                    } else {
                        win.fullscreen();
                    }
                }

                return Propagation::Stop;
            }

            // Esc closes menu panels first, then the findbar, otherwise
            // stops loading.
            if key == gdk::Key::Escape {
                let panels_open = toolbar_slot.borrow().as_ref().is_some_and(|toolbar| {
                    toolbar.menu_panel.is_visible() || toolbar.downloads_panel.is_visible()
                }) || bookmark_bar_slot
                    .borrow()
                    .as_ref()
                    .is_some_and(|bar| bar.overflow_panel.is_visible());

                if panels_open {
                    if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                        toolbar.close_panels();
                    }

                    if let Some(bar) = bookmark_bar_slot.borrow().as_ref() {
                        bar.close_panels();
                    }

                    return Propagation::Stop;
                }

                if let Some(findbar) = findbar_slot.borrow().as_ref()
                    && findbar.is_visible()
                {
                    findbar.hide();

                    return Propagation::Stop;
                }

                if let Some(view) = current_web_view.borrow().as_ref()
                    && view.is_loading()
                {
                    view.stop_loading();

                    return Propagation::Stop;
                }
            }

            Propagation::Proceed
        });

        window.add_controller(keyboard);
    }
}
