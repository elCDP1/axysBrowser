use gtk::prelude::*;
use gtk::{
    Align, Box, Button, CenterBox, Entry, Image, MenuButton, Orientation, Popover, Spinner, Stack,
    Window,
};
use std::cell::RefCell;
use std::rc::Rc;
use webkit6::{WebView, prelude::WebViewExt};

use super::menu::{build_menu, build_menu_model};

use crate::app_state::AppState;
use crate::browser::address::{SearchConfig, engine_display_name};
use crate::browser::bookmarks::Bookmark;
use crate::browser::downloads::DownloadStatus;
use crate::internal::pages::downloads::build_row;

type Action = Rc<dyn Fn()>;

/// Refreshes the address placeholder with the current default engine name.
fn update_placeholder(address: &Entry, search: &Rc<RefCell<SearchConfig>>) {
    let engine = search.borrow().default_engine.clone();
    let label = engine_display_name(&engine);

    address.set_placeholder_text(Some(&rust_i18n::t!("app.enter_url", engine = label)));
}

fn bookmark_dialog(
    parent: &gtk::Widget,
    state: &Rc<AppState>,
    existing: Option<Bookmark>,
    current_url: String,
    current_title: String,
    current_web_view: Rc<RefCell<Option<WebView>>>,
    refresh_star: Rc<dyn Fn()>,
) {
    let dialog = Window::builder()
        .modal(true)
        .title(if existing.is_some() {
            rust_i18n::t!("bookmarks.edit")
        } else {
            rust_i18n::t!("bookmarks.add")
        })
        .default_width(460)
        .default_height(220)
        .build();

    if let Some(root) = parent.root()
        && let Ok(parent_window) = root.downcast::<Window>()
    {
        dialog.set_transient_for(Some(&parent_window));
    }

    let content = Box::new(Orientation::Vertical, 12);

    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let name_label = gtk::Label::new(Some(&rust_i18n::t!("bookmarks.name")));
    name_label.set_halign(Align::Start);

    let name = Entry::new();
    name.set_hexpand(true);

    let url_label = gtk::Label::new(Some(&rust_i18n::t!("bookmarks.url")));
    url_label.set_halign(Align::Start);

    let url = Entry::new();
    url.set_hexpand(true);

    let initial_title = existing
        .as_ref()
        .map(|bookmark| bookmark.title.clone())
        .unwrap_or(current_title);

    let initial_url = existing
        .as_ref()
        .map(|bookmark| bookmark.url.clone())
        .unwrap_or(current_url);

    name.set_text(&initial_title);
    url.set_text(&initial_url);

    content.append(&name_label);
    content.append(&name);
    content.append(&url_label);
    content.append(&url);

    let buttons = Box::new(Orientation::Horizontal, 8);
    buttons.set_halign(Align::End);

    let cancel = Button::with_label(&rust_i18n::t!("bookmarks.cancel"));
    cancel.add_css_class("flat");

    let save = Button::with_label(&rust_i18n::t!("bookmarks.save"));
    save.add_css_class("suggested-action");

    let remove = Button::with_label(&rust_i18n::t!("bookmarks.remove"));
    remove.add_css_class("destructive-action");
    remove.set_visible(existing.is_some());

    buttons.append(&remove);
    buttons.append(&cancel);
    buttons.append(&save);

    content.append(&buttons);

    {
        let dialog = dialog.clone();

        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }

    {
        let state = state.clone();
        let dialog = dialog.clone();
        let name = name.clone();
        let url = url.clone();
        let existing = existing.clone();
        let current_web_view = current_web_view.clone();
        let refresh_star = refresh_star.clone();

        save.connect_clicked(move |_| {
            let title = name.text().trim().to_string();
            let target = url.text().trim().to_string();

            if target.is_empty()
                || !(target.starts_with("http://") || target.starts_with("https://"))
            {
                url.add_css_class("error");
                return;
            }

            let favicon = current_web_view
                .borrow()
                .as_ref()
                .and_then(|view| view.favicon());

            let success = if let Some(bookmark) = existing.as_ref() {
                state
                    .bookmarks
                    .update_with_favicon(&bookmark.url, title, target, favicon.as_ref())
            } else {
                state
                    .bookmarks
                    .add_with_favicon(title, target, favicon.as_ref())
            };

            if success {
                refresh_star();
                dialog.close();
            } else {
                url.add_css_class("error");
            }
        });
    }

    {
        let state = state.clone();
        let dialog = dialog.clone();
        let refresh_star = refresh_star.clone();
        let existing = existing.clone();

        remove.connect_clicked(move |_| {
            if let Some(bookmark) = existing.as_ref() {
                state.bookmarks.remove(&bookmark.url);
                refresh_star();
                dialog.close();
            }
        });
    }

    dialog.set_child(Some(&content));
    dialog.present();
}

pub struct Toolbar {
    pub root: CenterBox,
    pub address: Entry,
    reload_stack: Stack,
    spinner: Spinner,
    back: Button,
    forward: Button,
    bookmark: Button,
    bookmark_refresh: Action,
    _downloads_refresh: Action,
    _language_refresh: Action,
    _placeholder_refresh: Action,
}

/// Window-scoped dependencies for [`Toolbar::new`], kept together so the
/// constructor stays readable as options grow.
pub struct ToolbarOptions {
    pub state: Rc<AppState>,
    pub search: Rc<RefCell<SearchConfig>>,
    pub private_mode: bool,
}

impl Toolbar {
    pub fn new(
        current_web_view: Rc<RefCell<Option<WebView>>>,
        on_navigate: Rc<dyn Fn(String)>,
        on_reload: Action,
        on_back: Action,
        on_forward: Action,
        options: ToolbarOptions,
    ) -> Self {
        let ToolbarOptions {
            state,
            search,
            private_mode,
        } = options;
        let toolbar = CenterBox::new();

        toolbar.set_margin_top(3);
        toolbar.set_margin_bottom(7);
        toolbar.set_margin_start(8);
        toolbar.set_margin_end(8);

        let navigation = Box::new(Orientation::Horizontal, 2);

        let back = Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text(rust_i18n::t!("app.back").as_ref())
            .build();

        let forward = Button::builder()
            .icon_name("go-next-symbolic")
            .tooltip_text(rust_i18n::t!("app.forward").as_ref())
            .build();

        let reload_stack = Stack::new();
        reload_stack.set_transition_type(gtk::StackTransitionType::None);

        let reload_image = Image::from_icon_name("view-refresh-symbolic");
        reload_image.set_pixel_size(16);

        let reload = Button::builder()
            .tooltip_text(rust_i18n::t!("app.reload").as_ref())
            .build();

        let spinner = Spinner::new();
        spinner.set_size_request(16, 16);

        reload_stack.add_named(&reload_image, Some("reload"));
        reload_stack.add_named(&spinner, Some("loading"));
        reload_stack.set_visible_child_name("reload");

        reload.set_child(Some(&reload_stack));

        back.add_css_class("flat");
        forward.add_css_class("flat");
        reload.add_css_class("flat");

        // Back/forward go through the window-level actions so they also work
        // on internal axys:// pages (which have no WebKit history).
        back.connect_clicked(move |_| {
            on_back();
        });

        forward.connect_clicked(move |_| {
            on_forward();
        });

        reload.connect_clicked(move |_| {
            on_reload();
        });

        navigation.append(&back);
        navigation.append(&forward);
        navigation.append(&reload);

        let address = Entry::builder().hexpand(true).build();

        address.add_css_class("address-top");

        update_placeholder(&address, &search);

        {
            let on_navigate = on_navigate.clone();

            address.connect_activate(move |entry| {
                on_navigate(entry.text().to_string());
            });
        }

        // Chromium-style address selection: focusing selects everything
        // (deferred to an idle so the focusing click places its cursor
        // first), but the text is never deleted. Typing replaces the
        // selection; a second click while focused just places the cursor
        // (GTK default, no focus event involved).
        let focus_controller = gtk::EventControllerFocus::new();

        {
            let address = address.clone();

            focus_controller.connect_enter(move |_| {
                let address = address.clone();

                gtk::glib::idle_add_local_once(move || {
                    address.select_region(0, -1);
                });
            });
        }

        address.add_controller(focus_controller);

        let right = Box::new(Orientation::Horizontal, 2);

        let bookmark = Button::builder()
            .icon_name("non-starred-symbolic")
            .tooltip_text(rust_i18n::t!("bookmarks.add").as_ref())
            .build();

        bookmark.add_css_class("flat");

        let bookmark_refresh: Action = {
            let bookmark = bookmark.clone();

            let current_web_view = current_web_view.clone();

            let manager = state.bookmarks.clone();

            Rc::new(move || {
                let current_web_view_ref = current_web_view.borrow();

                let Some(view) = current_web_view_ref.as_ref() else {
                    bookmark.set_icon_name("non-starred-symbolic");

                    bookmark.set_tooltip_text(Some(rust_i18n::t!("bookmarks.add").as_ref()));

                    return;
                };

                let Some(uri) = view.uri().map(|uri| uri.to_string()) else {
                    bookmark.set_icon_name("non-starred-symbolic");

                    bookmark.set_tooltip_text(Some(rust_i18n::t!("bookmarks.add").as_ref()));

                    return;
                };

                if manager.contains(&uri) {
                    bookmark.set_icon_name("starred-symbolic");

                    bookmark.set_tooltip_text(Some(rust_i18n::t!("bookmarks.edit").as_ref()));
                } else {
                    bookmark.set_icon_name("non-starred-symbolic");

                    bookmark.set_tooltip_text(Some(rust_i18n::t!("bookmarks.add").as_ref()));
                }
            })
        };

        state.bookmarks.subscribe(&bookmark_refresh);

        {
            let state = state.clone();
            let current_web_view = current_web_view.clone();
            let parent = toolbar.clone();
            let refresh_star = bookmark_refresh.clone();

            bookmark.connect_clicked(move |_| {
                let current_web_view_ref = current_web_view.borrow();

                let Some(view) = current_web_view_ref.as_ref() else {
                    return;
                };

                let Some(uri) = view.uri().map(|uri| uri.to_string()) else {
                    return;
                };

                if !(uri.starts_with("http://") || uri.starts_with("https://")) {
                    return;
                }

                let title = view
                    .title()
                    .map(|title| title.to_string())
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| uri.clone());

                let existing = state.bookmarks.get(&uri);

                bookmark_dialog(
                    parent.upcast_ref(),
                    &state,
                    existing,
                    uri,
                    title,
                    current_web_view.clone(),
                    refresh_star.clone(),
                );
            });
        }

        let downloads_popover = Popover::new();

        let downloads_content = Box::new(Orientation::Vertical, 8);

        downloads_content.set_margin_top(10);
        downloads_content.set_margin_bottom(10);
        downloads_content.set_margin_start(10);
        downloads_content.set_margin_end(10);
        downloads_content.set_size_request(280, -1);

        downloads_popover.set_child(Some(&downloads_content));

        let downloads = MenuButton::builder()
            .icon_name("folder-download-symbolic")
            .tooltip_text(rust_i18n::t!("app.downloads").as_ref())
            .popover(&downloads_popover)
            .build();

        downloads.add_css_class("flat");
        downloads.set_visible(false);

        // Same stuck-toggle insurance as the main menu (see ui::menu).
        {
            let back = downloads.clone();

            downloads_popover.connect_closed(move |_| {
                back.set_active(false);
            });
        }

        let menu = build_menu(private_mode);
        menu.add_css_class("flat");

        right.append(&bookmark);
        right.append(&downloads);
        right.append(&menu);

        toolbar.set_start_widget(Some(&navigation));
        toolbar.set_center_widget(Some(&address));
        toolbar.set_end_widget(Some(&right));

        let downloads_refresh: Action = {
            let downloads_button = downloads.clone();

            let downloads_content = downloads_content.clone();

            let downloads_popover = downloads_popover.clone();

            let manager = state.downloads.clone();

            let on_navigate = on_navigate.clone();

            Rc::new(move || {
                let entries = manager.entries();

                let active = entries
                    .iter()
                    .filter(|entry| entry.status == DownloadStatus::InProgress)
                    .count();

                downloads_button.set_visible(!entries.is_empty());

                let tooltip = if active > 0 {
                    rust_i18n::t!("downloads.active_tooltip", count = active).to_string()
                } else {
                    rust_i18n::t!("app.downloads").to_string()
                };

                downloads_button.set_tooltip_text(Some(&tooltip));

                while let Some(child) = downloads_content.first_child() {
                    downloads_content.remove(&child);
                }

                let header = Box::new(Orientation::Horizontal, 8);

                header.set_hexpand(true);

                let title = gtk::Label::new(Some(&rust_i18n::t!("downloads.title")));

                title.add_css_class("title-4");

                title.set_halign(Align::Start);

                title.set_hexpand(true);

                header.append(&title);

                // Chromium-style "Clear all" for finished entries.
                let has_finished = entries.iter().any(|entry| entry.is_finished());

                if has_finished {
                    let clear = Button::with_label(&rust_i18n::t!("downloads.clear_finished"));

                    clear.add_css_class("flat");

                    {
                        let manager = manager.clone();

                        clear.connect_clicked(move |_| {
                            manager.clear_finished();
                        });
                    }

                    header.append(&clear);
                }

                downloads_content.append(&header);

                for entry in entries.iter().rev().take(5) {
                    downloads_content.append(&build_row(entry, &manager));
                }

                let see_all = Button::with_label(&rust_i18n::t!("downloads.see_all"));

                see_all.add_css_class("flat");

                see_all.set_halign(Align::Fill);

                {
                    let on_navigate = on_navigate.clone();

                    let downloads_popover = downloads_popover.clone();

                    see_all.connect_clicked(move |_| {
                        downloads_popover.popdown();

                        on_navigate("axys://downloads".to_string());
                    });
                }

                downloads_content.append(&see_all);
            })
        };

        downloads_refresh();

        state.downloads.subscribe(&downloads_refresh);

        bookmark_refresh();

        let language_refresh: Action = {
            let back = back.clone();

            let forward = forward.clone();

            let reload = reload.clone();

            let address = address.clone();

            let downloads = downloads.clone();

            let menu = menu.clone();

            let downloads_refresh = downloads_refresh.clone();

            let bookmark_refresh = bookmark_refresh.clone();

            let search = search.clone();

            Rc::new(move || {
                back.set_tooltip_text(Some(rust_i18n::t!("app.back").as_ref()));

                forward.set_tooltip_text(Some(rust_i18n::t!("app.forward").as_ref()));

                reload.set_tooltip_text(Some(rust_i18n::t!("app.reload").as_ref()));

                update_placeholder(&address, &search);

                downloads.set_tooltip_text(Some(rust_i18n::t!("app.downloads").as_ref()));

                bookmark_refresh();

                menu.set_tooltip_text(Some(rust_i18n::t!("app.menu").as_ref()));

                menu.set_menu_model(Some(&build_menu_model(private_mode)));

                downloads_refresh();
            })
        };

        state.subscribe_language(&language_refresh);

        // Engine changes (Settings/Welcome) update the placeholder live.
        let placeholder_refresh: Action = {
            let address = address.clone();
            let search = search.clone();

            Rc::new(move || {
                update_placeholder(&address, &search);
            })
        };

        state.subscribe_settings(&placeholder_refresh);

        Self {
            root: toolbar,
            address,
            reload_stack,
            spinner,
            back,
            forward,
            bookmark,
            bookmark_refresh: bookmark_refresh.clone(),
            _downloads_refresh: downloads_refresh,
            _language_refresh: language_refresh,
            _placeholder_refresh: placeholder_refresh,
        }
    }

    /// Refreshes the bookmark star for the currently shown view. Called on
    /// navigation since the star depends on the active URI, not just on
    /// bookmark list changes.
    pub fn refresh_bookmark(&self) {
        (self.bookmark_refresh)();
    }

    pub fn set_loading(&self, loading: bool) {
        if loading {
            self.reload_stack.set_visible_child_name("loading");

            self.spinner.start();
        } else {
            self.spinner.stop();

            self.reload_stack.set_visible_child_name("reload");
        }
    }

    pub fn set_navigation_state(&self, can_go_back: bool, can_go_forward: bool) {
        self.back.set_sensitive(can_go_back);

        self.forward.set_sensitive(can_go_forward);
    }

    pub fn activate_bookmark(&self) {
        self.bookmark.emit_clicked();
    }
}
