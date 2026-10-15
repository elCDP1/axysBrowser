use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box, Orientation, Stack,
    StackTransitionType, gdk, gio, glib::Propagation, style_context_add_provider_for_display,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use webkit6::{LoadEvent, NetworkSession, WebView, prelude::WebViewExt};

use crate::app_state::AppState;
use crate::browser::address::{SearchConfig, resolve_input};
use crate::browser::engine::BrowserEngine;
use crate::browser::tabs::{NewTabCallback, Tab};
use crate::internal::router;

use super::actions::{WindowActions, register_window_actions};
use super::bookmarks::BookmarkBar;
use super::findbar::FindBar;
use super::navigation::{address_text_for, begin_web_load};
use super::tabs::TabBar;
use super::toolbar::{Toolbar, ToolbarOptions};

fn maximize_window(window: &ApplicationWindow) {
    window.maximize();
}

pub fn build_window(app: &Application, state: Rc<AppState>) {
    let session = state.network_session.clone();
    let search = state.search.clone();

    build_browser_window(app, state, session, search, false);
}

pub fn build_private_window(app: &Application, state: Rc<AppState>) {
    let session = BrowserEngine::create_ephemeral_session();

    state.downloads.watch(&session);

    let mut private_search = state.search.borrow().clone();

    if private_search.engines.contains_key("brave") {
        private_search.default_engine = "brave".to_string();
    }

    let search = Rc::new(RefCell::new(private_search));

    build_browser_window(app, state, session, search, true);
}

fn build_browser_window(
    app: &Application,
    state: Rc<AppState>,
    session: NetworkSession,
    search: Rc<RefCell<SearchConfig>>,
    private_mode: bool,
) {
    let window_title = if private_mode {
        rust_i18n::t!("privacy.title").to_string()
    } else {
        "axysBrowser".to_string()
    };

    let window = ApplicationWindow::builder()
        .application(app)
        .title(window_title)
        .icon_name("axysbrowser")
        .resizable(true)
        .default_width(1100)
        .default_height(720)
        .build();

    // Private windows have no banner; this class drives a visible (purple)
    // accent so they can never be mistaken for a normal window.
    if private_mode {
        window.add_css_class("private-window");
    }

    if !state.css_installed.get()
        && let Some(display) = gdk::Display::default()
    {
        style_context_add_provider_for_display(
            &display,
            &state.css_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        state.css_installed.set(true);
    }

    let root = Box::new(Orientation::Vertical, 0);

    root.set_hexpand(true);
    root.set_vexpand(true);

    // Note: private mode is a window mode toggled from the menu — there is
    // deliberately no top banner (it wasted space and clipped its text).

    let tabs = Rc::new(RefCell::new(Vec::<Tab>::new()));

    let active_id = Rc::new(Cell::new(usize::MAX));
    let next_id = Rc::new(Cell::new(0usize));

    /// URIs (plus titles) of recently closed tabs, for Ctrl+Shift+T reopen.
    /// Per window, so private restores stay inside the private window.
    const MAX_CLOSED_TABS: usize = 25;

    let closed_tabs = Rc::new(RefCell::new(Vec::<(String, String)>::new()));

    let top_stack = Stack::new();

    top_stack.set_hexpand(true);
    top_stack.set_vexpand(true);
    top_stack.set_halign(gtk::Align::Fill);
    top_stack.set_valign(gtk::Align::Fill);
    top_stack.set_transition_type(StackTransitionType::None);

    let current_web_view = Rc::new(RefCell::new(None::<WebView>));

    let tabbar_slot: Rc<RefCell<Option<TabBar>>> = Rc::new(RefCell::new(None));

    let toolbar_slot: Rc<RefCell<Option<Toolbar>>> = Rc::new(RefCell::new(None));

    let bookmark_bar_slot: Rc<RefCell<Option<BookmarkBar>>> = Rc::new(RefCell::new(None));

    let findbar_slot: Rc<RefCell<Option<FindBar>>> = Rc::new(RefCell::new(None));

    // Invisible full-size click-catcher for the menu panels (see ui::menu):
    // visible while any panel is open, so any click outside a panel closes
    // everything instead of leaving it hidden under the UI.
    let catcher = gtk::Button::new();

    catcher.set_hexpand(true);
    catcher.set_vexpand(true);
    catcher.set_opacity(0.0);
    catcher.set_can_focus(false);
    catcher.set_visible(false);

    let sync_panels: Rc<dyn Fn()> = {
        let toolbar_slot = toolbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();
        let catcher = catcher.clone();

        Rc::new(move || {
            let any_open = toolbar_slot.borrow().as_ref().is_some_and(|toolbar| {
                toolbar.menu_panel.is_visible() || toolbar.downloads_panel.is_visible()
            }) || bookmark_bar_slot.borrow().as_ref().is_some_and(|bar| {
                bar.overflow_panel.is_visible()
            });

            catcher.set_visible(any_open);
        })
    };

    let select_tab: Rc<dyn Fn(usize)> = {
        let tabs = tabs.clone();
        let active_id = active_id.clone();
        let top_stack = top_stack.clone();
        let current_web_view = current_web_view.clone();
        let toolbar_slot = toolbar_slot.clone();
        let tabbar_slot = tabbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();
        let findbar_slot = findbar_slot.clone();

        Rc::new(move |id| {
            let tab = {
                let tabs = tabs.borrow();

                tabs.iter().find(|tab| tab.id == id).map(|tab| {
                    (
                        tab.content.clone(),
                        tab.web_view.clone(),
                        tab.uri.clone(),
                        tab.can_go_back(),
                        tab.can_go_forward(),
                    )
                })
            };

            let Some((content, web_view, uri, can_go_back, can_go_forward)) = tab else {
                return;
            };

            active_id.set(id);

            top_stack.set_visible_child(&content);

            // Clear the previous tab's find highlights before switching.
            if let Some(findbar) = findbar_slot.borrow().as_ref()
                && let Some(previous) = current_web_view.borrow().as_ref()
            {
                findbar.stop_in(previous);
            }

            // Switching tabs always dismisses menu panels.
            if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                toolbar.close_panels();
            }

            if let Some(bar) = bookmark_bar_slot.borrow().as_ref() {
                bar.close_panels();
            }

            *current_web_view.borrow_mut() = Some(web_view.clone());

            // Keep an open findbar searching in the newly shown tab.
            if let Some(findbar) = findbar_slot.borrow().as_ref() {
                findbar.research_in(&web_view);
            }

            if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                toolbar.address.set_text(address_text_for(&uri));

                toolbar.set_loading(web_view.is_loading());

                toolbar.set_navigation_state(
                    can_go_back || web_view.can_go_back(),
                    can_go_forward || web_view.can_go_forward(),
                );

                toolbar.refresh_bookmark();
            }

            if let Some(tabbar) = tabbar_slot.borrow().as_ref() {
                tabbar.refresh(&tabs.borrow(), id);
            }

            if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                bookmark_bar.refresh();
            }
        })
    };

    let close_tab: Rc<dyn Fn(usize)> = {
        let tabs = tabs.clone();
        let active_id = active_id.clone();
        let top_stack = top_stack.clone();
        let toolbar_slot = toolbar_slot.clone();
        let tabbar_slot = tabbar_slot.clone();
        let current_web_view = current_web_view.clone();
        let select_tab = select_tab.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();
        let closed_tabs = closed_tabs.clone();
        let findbar_slot = findbar_slot.clone();

        Rc::new(move |id| {
            if tabs.borrow().len() == 1 {
                let (content, web_view) = {
                    let mut tabs_ref = tabs.borrow_mut();

                    let Some(tab) = tabs_ref.iter_mut().find(|tab| tab.id == id) else {
                        return;
                    };

                    tab.title = rust_i18n::t!("app.new_tab").to_string();

                    tab.uri = "axys://newtab".to_string();

                    tab.history = vec!["axys://newtab".to_string()];

                    tab.history_index = 0;

                    tab.content.set_visible_child_name("newtab");

                    (tab.content.clone(), tab.web_view.clone())
                };

                active_id.set(id);

                top_stack.set_visible_child(&content);

                // Don't leave a heavy page loading in a hidden view, and
                // release the old page's resources: this WebView is reused
                // for the fresh tab, so unload explicitly.
                web_view.stop_loading();
                web_view.load_html("", None);

                *current_web_view.borrow_mut() = Some(web_view);

                if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                    toolbar.address.set_text("axys://newtab");

                    toolbar.set_loading(false);

                    toolbar.set_navigation_state(false, false);
                }

                if let Some(tabbar) = tabbar_slot.borrow().as_ref() {
                    tabbar.refresh(&tabs.borrow(), id);
                }

                if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                    bookmark_bar.refresh();
                }

                return;
            }

            let was_active = active_id.get() == id;

            let removed = {
                let mut tabs_ref = tabs.borrow_mut();

                tabs_ref
                    .iter()
                    .position(|tab| tab.id == id)
                    .map(|index| tabs_ref.remove(index))
            };

            let Some(removed) = removed else {
                return;
            };

            top_stack.remove(&removed.content);

            removed.web_view.stop_loading();

            // Drop find hooks so the closed tab's controller (and view)
            // can be freed; they re-hook lazily on the next search.
            if let Some(findbar) = findbar_slot.borrow().as_ref() {
                findbar.forget_hooks();
            }

            // Remember for Ctrl+Shift+T reopen (skip blank new tabs).
            if removed.uri != "axys://newtab" {
                let mut closed = closed_tabs.borrow_mut();

                closed.push((removed.uri.clone(), removed.title.clone()));

                if closed.len() > MAX_CLOSED_TABS {
                    closed.remove(0);
                }
            }

            if was_active {
                let new_id = tabs.borrow().last().map(|tab| tab.id);

                if let Some(new_id) = new_id {
                    select_tab(new_id);
                }
            }

            if let Some(tabbar) = tabbar_slot.borrow().as_ref() {
                tabbar.refresh(&tabs.borrow(), active_id.get());
            }

            if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                bookmark_bar.refresh();
            }
        })
    };

    let navigate_tab: Rc<dyn Fn(usize, String)> = {
        let tabs = tabs.clone();
        let top_stack = top_stack.clone();
        let active_id = active_id.clone();
        let toolbar_slot = toolbar_slot.clone();
        let tabbar_slot = tabbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();
        let search = search.clone();

        Rc::new(move |id, input| {
            let target = {
                let config = search.borrow();

                resolve_input(&input, &config)
            };

            if let Some(page) = router::page_name(&target) {
                let (content, can_go_back, can_go_forward) = {
                    let mut tabs_ref = tabs.borrow_mut();

                    let Some(tab) = tabs_ref.iter_mut().find(|tab| tab.id == id) else {
                        return;
                    };

                    tab.content.set_visible_child_name(page);

                    if tab.uri.starts_with("http://") || tab.uri.starts_with("https://") {
                        let current = tab.uri.clone();

                        tab.push_history(current);
                    }

                    tab.uri = target.clone();

                    tab.title = router::page_title(&target);

                    tab.push_history(target.clone());

                    (tab.content.clone(), tab.can_go_back(), tab.can_go_forward())
                };

                if let Some(tabbar) = tabbar_slot.borrow().as_ref() {
                    tabbar.refresh(&tabs.borrow(), active_id.get());
                }

                if active_id.get() == id {
                    top_stack.set_visible_child(&content);

                    if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                        toolbar.address.set_text(address_text_for(&target));

                        toolbar.set_loading(false);

                        toolbar.set_navigation_state(can_go_back, can_go_forward);
                    }

                    if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                        bookmark_bar.refresh();
                    }
                }

                return;
            }

            if !target.starts_with("http://")
                && !target.starts_with("https://")
                && !target.starts_with("file://")
                && target != "about:blank"
            {
                return;
            }

            let (content, web_view, can_go_back, can_go_forward) = {
                let mut tabs_ref = tabs.borrow_mut();

                let Some(tab) = tabs_ref.iter_mut().find(|tab| tab.id == id) else {
                    return;
                };

                if tab.uri != target {
                    tab.push_history(target.clone());
                }

                tab.uri = target.clone();

                tab.content.set_visible_child_name("web");

                (
                    tab.content.clone(),
                    tab.web_view.clone(),
                    tab.can_go_back(),
                    tab.can_go_forward(),
                )
            };

            if active_id.get() == id {
                top_stack.set_visible_child(&content);

                if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                    toolbar.address.set_text(address_text_for(&target));

                    toolbar.set_loading(true);

                    toolbar.set_navigation_state(
                        can_go_back,
                        can_go_forward || web_view.can_go_forward(),
                    );
                }

                if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                    bookmark_bar.refresh();
                }
            }

            BrowserEngine::load(&web_view, &target);

            begin_web_load(&tabs, id, &web_view, &target);
        })
    };

    let go_back: Rc<dyn Fn()> = {
        let tabs = tabs.clone();
        let active_id = active_id.clone();
        let top_stack = top_stack.clone();
        let toolbar_slot = toolbar_slot.clone();
        let tabbar_slot = tabbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();
        let current_web_view = current_web_view.clone();

        Rc::new(move || {
            let id = active_id.get();

            // For real web pages (e.g. Discord, an SPA using pushState),
            // WebKit's own back-stack is authoritative. A full load_uri here
            // would discard all JS state and force a heavy reload that can
            // OOM the web process — so prefer native navigation and only use
            // the custom history for internal axys:// pages or when WebKit
            // has nothing to go back to.
            let on_web_page = tabs
                .borrow()
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| router::page_name(&tab.uri).is_none())
                .unwrap_or(false);

            if on_web_page
                && let Some(view) = current_web_view.borrow().as_ref()
                && view.can_go_back()
            {
                // The custom history only tracks the entries *before* the web
                // view (internal pages); touching it here desynchronised it
                // from WebKit's stack and made the first entry unreachable.
                view.go_back();

                return;
            }

            let target = {
                let mut tabs_ref = tabs.borrow_mut();

                let Some(tab) = tabs_ref.iter_mut().find(|tab| tab.id == id) else {
                    return;
                };

                tab.go_back()
            };

            let Some(target) = target else {
                return;
            };

            if let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id) {
                tab.uri = target.clone();

                if router::page_name(&target).is_some() {
                    tab.title = router::page_title(&target);
                }
            }

            let tab_data = {
                let tabs_ref = tabs.borrow();

                tabs_ref.iter().find(|tab| tab.id == id).map(|tab| {
                    (
                        tab.content.clone(),
                        tab.web_view.clone(),
                        tab.can_go_back(),
                        tab.can_go_forward(),
                    )
                })
            };

            let Some((content, web_view, can_go_back, can_go_forward)) = tab_data else {
                return;
            };

            if let Some(page) = router::page_name(&target) {
                content.set_visible_child_name(page);

                if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                    toolbar.address.set_text(address_text_for(&target));

                    toolbar.set_loading(false);

                    toolbar.set_navigation_state(can_go_back, can_go_forward);
                }

                if let Some(tabbar) = tabbar_slot.borrow().as_ref() {
                    tabbar.refresh(&tabs.borrow(), active_id.get());
                }

                if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                    bookmark_bar.refresh();
                }

                top_stack.set_visible_child(&content);
            } else {
                BrowserEngine::load(&web_view, &target);

                begin_web_load(&tabs, id, &web_view, &target);
            }
        })
    };

    let go_forward: Rc<dyn Fn()> = {
        let tabs = tabs.clone();
        let active_id = active_id.clone();
        let top_stack = top_stack.clone();
        let toolbar_slot = toolbar_slot.clone();
        let tabbar_slot = tabbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();
        let current_web_view = current_web_view.clone();

        Rc::new(move || {
            let id = active_id.get();

            let on_web_page = tabs
                .borrow()
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| router::page_name(&tab.uri).is_none())
                .unwrap_or(false);

            if on_web_page
                && let Some(view) = current_web_view.borrow().as_ref()
                && view.can_go_forward()
            {
                view.go_forward();

                return;
            }

            let target = {
                let mut tabs_ref = tabs.borrow_mut();

                let Some(tab) = tabs_ref.iter_mut().find(|tab| tab.id == id) else {
                    return;
                };

                tab.go_forward()
            };

            let Some(target) = target else {
                return;
            };

            if let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id) {
                tab.uri = target.clone();

                if router::page_name(&target).is_some() {
                    tab.title = router::page_title(&target);
                }
            }

            let tab_data = {
                let tabs_ref = tabs.borrow();

                tabs_ref.iter().find(|tab| tab.id == id).map(|tab| {
                    (
                        tab.content.clone(),
                        tab.web_view.clone(),
                        tab.can_go_back(),
                        tab.can_go_forward(),
                    )
                })
            };

            let Some((content, web_view, can_go_back, can_go_forward)) = tab_data else {
                return;
            };

            if let Some(page) = router::page_name(&target) {
                content.set_visible_child_name(page);

                if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                    toolbar.address.set_text(address_text_for(&target));

                    toolbar.set_loading(false);

                    toolbar.set_navigation_state(can_go_back, can_go_forward);
                }

                if let Some(tabbar) = tabbar_slot.borrow().as_ref() {
                    tabbar.refresh(&tabs.borrow(), active_id.get());
                }

                if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                    bookmark_bar.refresh();
                }

                top_stack.set_visible_child(&content);
            } else {
                BrowserEngine::load(&web_view, &target);

                begin_web_load(&tabs, id, &web_view, &target);
            }
        })
    };

    // Popup opener with late binding: `new_tab` needs it at creation time,
    // but it needs `new_tab` to open the URL. Resolved via slot + idle to
    // also avoid re-entrancy inside WebKit's `create` signal.
    let new_tab_slot: Rc<RefCell<Option<NewTabCallback>>> = Rc::new(RefCell::new(None));

    let open_in_new_tab: Rc<dyn Fn(String)> = {
        // Weak: new_tab owns this closure, and the slot owns new_tab.
        let new_tab_slot = Rc::downgrade(&new_tab_slot);

        Rc::new(move |url: String| {
            let new_tab_slot = new_tab_slot.clone();

            gtk::glib::idle_add_local_once(move || {
                if let Some(slot) = new_tab_slot.upgrade()
                    && let Some(new_tab) = slot.borrow().clone()
                {
                    new_tab(Some(&url));
                }
            });
        })
    };

    // Forwards web notifications (Discord mentions, …) to the desktop.
    let show_notification: Rc<dyn Fn(String, String)> = {
        let app = app.clone();
        let next_id = Rc::new(Cell::new(0u64));

        Rc::new(move |title: String, body: String| {
            let id = next_id.get();

            next_id.set(id + 1);

            let notification = gio::Notification::new(&title);

            if !body.trim().is_empty() {
                notification.set_body(Some(&body));
            }

            app.send_notification(Some(&format!("axys-web-{id}")), &notification);
        })
    };

    let new_tab: NewTabCallback = {
        let tabs = tabs.clone();
        let next_id = next_id.clone();
        let active_id = active_id.clone();
        let top_stack = top_stack.clone();
        let current_web_view = current_web_view.clone();

        let tabbar_slot = Rc::downgrade(&tabbar_slot);
        let toolbar_slot = Rc::downgrade(&toolbar_slot);
        let bookmark_bar_slot = Rc::downgrade(&bookmark_bar_slot);

        let navigate_tab = navigate_tab.clone();
        let open_in_new_tab = open_in_new_tab.clone();
        let show_notification = show_notification.clone();
        let state = state.clone();
        let session = session.clone();

        Rc::new(move |starting_uri: Option<&str>| {
            let id = next_id.get();

            next_id.set(id + 1);

            let content = Stack::new();

            content.set_hexpand(true);
            content.set_vexpand(true);
            content.set_halign(gtk::Align::Fill);
            content.set_valign(gtk::Align::Fill);
            content.set_transition_type(StackTransitionType::None);

            let web_view = WebView::builder().network_session(&session).build();

            web_view.set_hexpand(true);
            web_view.set_vexpand(true);
            web_view.set_halign(gtk::Align::Fill);
            web_view.set_valign(gtk::Align::Fill);

            BrowserEngine::configure(
                &web_view,
                state.settings.borrow().developer_tools,
                state.settings.borrow().hardware_acceleration,
            );

            let permissions = if private_mode {
                // Private grants live only as long as this window.
                crate::browser::permissions::PermissionManager::ephemeral()
            } else {
                state.permissions.clone()
            };

            BrowserEngine::wire_view(
                &web_view,
                open_in_new_tab.clone(),
                show_notification.clone(),
                permissions,
            );

            if private_mode
                && let Some(settings) = webkit6::prelude::WebViewExt::settings(&web_view)
            {
                settings.set_enable_developer_extras(false);
            }

            content.add_named(&web_view, Some("web"));

            let local_search = {
                let navigate_tab = navigate_tab.clone();

                Rc::new(move |input: String| {
                    navigate_tab(id, input);
                })
            };

            let local_about = {
                let navigate_tab = navigate_tab.clone();

                Rc::new(move || {
                    navigate_tab(id, "axys://about".to_string());
                })
            };

            let local_downloads = {
                let navigate_tab = navigate_tab.clone();

                Rc::new(move || {
                    navigate_tab(id, "axys://downloads".to_string());
                })
            };

            let local_history = {
                let navigate_tab = navigate_tab.clone();

                Rc::new(move || {
                    navigate_tab(id, "axys://history".to_string());
                })
            };

            let local_clear_browsing_data = {
                let state = state.clone();

                Rc::new(move || {
                    state.clear_browsing_data();
                })
            };

            for uri in [
                "axys://welcome",
                "axys://newtab",
                "axys://privacy",
                "axys://about",
                "axys://settings",
                "axys://tools",
                "axys://downloads",
                "axys://history",
            ] {
                if let Some(widget) = router::route(
                    uri,
                    local_search.clone(),
                    local_about.clone(),
                    local_downloads.clone(),
                    local_history.clone(),
                    local_clear_browsing_data.clone(),
                    state.clone(),
                ) && let Some(name) = router::page_name(uri)
                {
                    content.add_named(&widget, Some(name));
                }
            }

            // Private windows open a normal new tab too; private mode is
            // indicated by the window title and menu, not a landing page.
            let first_uri = starting_uri
                .map(|uri| uri.to_string())
                .unwrap_or_else(|| "axys://newtab".to_string());

            let first_page = router::page_name(&first_uri).unwrap_or("newtab");

            content.set_visible_child_name(first_page);

            let tab_name = format!("tab-{id}");

            top_stack.add_named(&content, Some(&tab_name));

            let mut tab = Tab::new(id, content.clone(), web_view.clone());

            tab.uri = first_uri.clone();

            tab.title = router::page_title(&first_uri);

            tab.history = vec![first_uri.clone()];

            tab.history_index = 0;

            tabs.borrow_mut().push(tab);

            active_id.set(id);

            top_stack.set_visible_child(&content);

            *current_web_view.borrow_mut() = Some(web_view.clone());

            if let Some(slot) = toolbar_slot.upgrade()
                && let Some(toolbar) = slot.borrow().as_ref()
            {
                toolbar.address.set_text(address_text_for(&first_uri));

                toolbar.set_loading(false);

                toolbar.set_navigation_state(false, false);
            }

            if let Some(slot) = bookmark_bar_slot.upgrade()
                && let Some(bookmark_bar) = slot.borrow().as_ref()
            {
                bookmark_bar.refresh();
            }

            {
                let tabs = tabs.clone();
                let active_id = active_id.clone();
                let toolbar_slot = toolbar_slot.clone();

                web_view.connect_uri_notify(move |view| {
                    let Some(uri) = view.uri().map(|uri| uri.to_string()) else {
                        return;
                    };

                    if let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id) {
                        tab.uri = uri.clone();
                    }

                    // Neither the tab bar (titles only) nor the bookmark bar
                    // (bookmark list) depend on the URI — just update the
                    // address and the bookmark star.
                    if active_id.get() == id
                        && let Some(slot) = toolbar_slot.upgrade()
                        && let Some(toolbar) = slot.borrow().as_ref()
                    {
                        toolbar.address.set_text(address_text_for(&uri));
                        toolbar.refresh_bookmark();
                    }
                });
            }

            {
                let tabs = tabs.clone();
                let active_id = active_id.clone();
                let tabbar_slot = tabbar_slot.clone();

                web_view.connect_title_notify(move |view| {
                    let title = view
                        .title()
                        .map(|title| title.to_string())
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or_else(|| {
                            if private_mode {
                                rust_i18n::t!("privacy.title").to_string()
                            } else {
                                rust_i18n::t!("app.new_tab").to_string()
                            }
                        });

                    if let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id) {
                        tab.title = title;
                    }

                    if let Some(slot) = tabbar_slot.upgrade()
                        && let Some(tabbar) = slot.borrow().as_ref()
                    {
                        tabbar.refresh(&tabs.borrow(), active_id.get());
                    }
                });
            }

            {
                let active_id = active_id.clone();
                let toolbar_slot = toolbar_slot.clone();

                web_view.connect_is_loading_notify(move |view| {
                    if active_id.get() != id {
                        return;
                    }

                    if let Some(slot) = toolbar_slot.upgrade()
                        && let Some(toolbar) = slot.borrow().as_ref()
                    {
                        toolbar.set_loading(view.is_loading());
                    }
                });
            }

            {
                let tabs = tabs.clone();

                web_view.connect_load_changed(move |_, event| {
                    if event == LoadEvent::Committed
                        && let Ok(mut tabs) = tabs.try_borrow_mut()
                        && let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id)
                    {
                        tab.committed = true;
                    }
                });
            }

            {
                let active_id = active_id.clone();
                let toolbar_slot = toolbar_slot.clone();
                let state = state.clone();

                web_view.connect_load_changed(move |view, event| {
                    // History is per-window, not per-visible-tab: a page
                    // finishing in the background must still be recorded.
                    if event == LoadEvent::Finished
                        && !private_mode
                        && let Some(uri) = view.uri().map(|uri| uri.to_string())
                        && (uri.starts_with("http://") || uri.starts_with("https://"))
                    {
                        let title = view
                            .title()
                            .map(|title| title.to_string())
                            .filter(|title| !title.trim().is_empty())
                            .unwrap_or_else(|| uri.clone());

                        state.history.add_visit(title, uri);
                    }

                    if active_id.get() != id {
                        return;
                    }

                    if let Some(slot) = toolbar_slot.upgrade()
                        && let Some(toolbar) = slot.borrow().as_ref()
                    {
                        toolbar.set_loading(view.is_loading());

                        match event {
                            LoadEvent::Started | LoadEvent::Committed | LoadEvent::Redirected => {
                                toolbar.set_navigation_state(
                                    view.can_go_back(),
                                    view.can_go_forward(),
                                );
                            }

                            LoadEvent::Finished => {
                                toolbar.set_navigation_state(
                                    view.can_go_back(),
                                    view.can_go_forward(),
                                );

                                toolbar.refresh_bookmark();
                            }

                            _ => {}
                        }
                    }
                });
            }

            if let Some(slot) = tabbar_slot.upgrade()
                && let Some(tabbar) = slot.borrow().as_ref()
            {
                tabbar.refresh(&tabs.borrow(), id);
            }

            id
        })
    };

    *new_tab_slot.borrow_mut() = Some(new_tab.clone());

    let reopen_closed_tab: Rc<dyn Fn()> = {
        let closed_tabs = closed_tabs.clone();
        let new_tab = new_tab.clone();
        let navigate_tab = navigate_tab.clone();

        Rc::new(move || {
            let Some((uri, _title)) = closed_tabs.borrow_mut().pop() else {
                return;
            };

            // Created with the URI so internal pages render; navigate again
            // so real web pages actually load (no duplicate history: the tab
            // already points at this URI).
            let id = new_tab(Some(&uri));

            navigate_tab(id, uri);
        })
    };

    let tabbar = TabBar::new(
        {
            let new_tab = new_tab.clone();

            Rc::new(move || {
                new_tab(None);
            })
        },
        select_tab.clone(),
        close_tab.clone(),
        {
            let navigate_tab = navigate_tab.clone();
            let active_id = active_id.clone();

            Rc::new(move || {
                navigate_tab(active_id.get(), "axys://about".to_string());
            })
        },
    );

    *tabbar_slot.borrow_mut() = Some(tabbar);

    let toolbar = Toolbar::new(
        current_web_view.clone(),
        {
            let navigate_tab = navigate_tab.clone();
            let active_id = active_id.clone();

            Rc::new(move |input: String| {
                navigate_tab(active_id.get(), input);
            })
        },
        {
            let tabs = tabs.clone();
            let active_id = active_id.clone();
            let navigate_tab = navigate_tab.clone();
            let current_web_view = current_web_view.clone();

            Rc::new(move || {
                let id = active_id.get();

                let uri = tabs
                    .borrow()
                    .iter()
                    .find(|tab| tab.id == id)
                    .map(|tab| tab.uri.clone());

                if let Some(uri) = uri
                    && router::page_name(&uri).is_some()
                {
                    navigate_tab(id, uri);

                    return;
                }

                if let Some(view) = current_web_view.borrow().as_ref() {
                    if view.is_loading() {
                        view.stop_loading();
                    } else {
                        // A reload is a fresh load for watchdog purposes.
                        if let Some(uri) = view.uri().map(|uri| uri.to_string()) {
                            begin_web_load(&tabs, id, view, &uri);
                        }

                        BrowserEngine::reload(view);
                    }
                }
            })
        },
        go_back.clone(),
        go_forward.clone(),
        ToolbarOptions {
            state: state.clone(),
            search: search.clone(),
            private_mode,
            on_panel_toggled: sync_panels.clone(),
        },
    );

    *toolbar_slot.borrow_mut() = Some(toolbar);

    let bookmark_bar = BookmarkBar::new(
        state.clone(),
        {
            let navigate_tab = navigate_tab.clone();
            let active_id = active_id.clone();

            Rc::new(move |url: String| {
                navigate_tab(active_id.get(), url);
            })
        },
        sync_panels.clone(),
    );

    *bookmark_bar_slot.borrow_mut() = Some(bookmark_bar);

    if let Some(tabbar) = tabbar_slot.borrow().as_ref() {
        root.append(&tabbar.root);
    }

    if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
        root.append(&toolbar.root);
    }

    if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
        root.append(&bookmark_bar.root);
    }

    // Menu panels live in an overlay above the page content: they paint as
    // normal widgets (immune to popup-surface bugs) and reflow with the
    // window, so they can never end up misplaced or hidden behind the UI.
    // (top_stack moves from root into the overlay here.)
    let content_overlay = gtk::Overlay::new();

    content_overlay.set_child(Some(&top_stack));

    content_overlay.add_overlay(&catcher);

    if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
        for panel in [
            &toolbar.menu_panel.container,
            &toolbar.downloads_panel,
        ] {
            panel.set_halign(gtk::Align::End);
            panel.set_valign(gtk::Align::Start);
            panel.set_margin_top(4);
            panel.set_margin_end(8);

            content_overlay.add_overlay(panel);
        }
    }

    if let Some(bar) = bookmark_bar_slot.borrow().as_ref() {
        let panel = &bar.overflow_panel;

        panel.set_halign(gtk::Align::End);
        panel.set_valign(gtk::Align::Start);
        panel.set_margin_top(4);
        panel.set_margin_end(8);

        content_overlay.add_overlay(panel);
    }

    {
        let toolbar_slot = toolbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();

        catcher.connect_clicked(move |_| {
            if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                toolbar.close_panels();
            }

            if let Some(bar) = bookmark_bar_slot.borrow().as_ref() {
                bar.close_panels();
            }
        });
    }

    root.append(&content_overlay);

    // Floating findbar lives in an overlay above the whole window content.
    let findbar = FindBar::new(current_web_view.clone());

    *findbar_slot.borrow_mut() = Some(findbar);

    let overlay = gtk::Overlay::new();

    overlay.set_child(Some(&root));

    if let Some(findbar) = findbar_slot.borrow().as_ref() {
        overlay.add_overlay(&findbar.container);
    }

    window.set_child(Some(&overlay));

    let window_weak = window.downgrade();

    let language_refresh: Rc<dyn Fn()> = {
        let tabs = tabs.clone();
        let active_id = active_id.clone();
        let top_stack = top_stack.clone();
        let toolbar_slot = toolbar_slot.clone();
        let tabbar_slot = tabbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();
        let navigate_tab = navigate_tab.clone();
        let state = state.clone();
        let window_weak = window_weak.clone();

        Rc::new(move || {
            let tab_data = {
                let tabs_ref = tabs.borrow();

                tabs_ref
                    .iter()
                    .map(|tab| (tab.id, tab.uri.clone(), tab.content.clone()))
                    .collect::<Vec<_>>()
            };

            for (id, uri, content) in tab_data {
                let Some(current_page) = router::page_name(&uri) else {
                    continue;
                };

                let local_search = {
                    let navigate_tab = navigate_tab.clone();

                    Rc::new(move |input: String| {
                        navigate_tab(id, input);
                    })
                };

                let local_about = {
                    let navigate_tab = navigate_tab.clone();

                    Rc::new(move || {
                        navigate_tab(id, "axys://about".to_string());
                    })
                };

                let local_downloads = {
                    let navigate_tab = navigate_tab.clone();

                    Rc::new(move || {
                        navigate_tab(id, "axys://downloads".to_string());
                    })
                };

                let local_history = {
                    let navigate_tab = navigate_tab.clone();

                    Rc::new(move || {
                        navigate_tab(id, "axys://history".to_string());
                    })
                };

                let local_clear_browsing_data = {
                    let state = state.clone();

                    Rc::new(move || {
                        state.clear_browsing_data();
                    })
                };

                for page_uri in [
                    "axys://welcome",
                    "axys://newtab",
                    "axys://privacy",
                    "axys://about",
                    "axys://settings",
                    "axys://tools",
                    "axys://downloads",
                    "axys://history",
                ] {
                    let Some(page_name) = router::page_name(page_uri) else {
                        continue;
                    };

                    let Some(widget) = router::route(
                        page_uri,
                        local_search.clone(),
                        local_about.clone(),
                        local_downloads.clone(),
                        local_history.clone(),
                        local_clear_browsing_data.clone(),
                        state.clone(),
                    ) else {
                        continue;
                    };

                    if let Some(old) = content.child_by_name(page_name) {
                        content.remove(&old);
                    }

                    content.add_named(&widget, Some(page_name));
                }

                content.set_visible_child_name(current_page);

                if let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id) {
                    tab.title = router::page_title(&uri);
                }
            }

            if let Some(tabbar) = tabbar_slot.borrow().as_ref() {
                tabbar.refresh(&tabs.borrow(), active_id.get());
            }

            if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                bookmark_bar.refresh();
            }

            if let Some(tab) = tabs.borrow().iter().find(|tab| tab.id == active_id.get()) {
                top_stack.set_visible_child(&tab.content);

                if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                    toolbar.address.set_text(address_text_for(&tab.uri));
                }
            }

            if let Some(window) = window_weak.upgrade() {
                if private_mode {
                    let title = rust_i18n::t!("privacy.title").to_string();

                    window.set_title(Some(&title));
                } else {
                    window.set_title(Some("axysBrowser"));
                }
            }
        })
    };

    state.subscribe_language(&language_refresh);

    // Live-apply developer tools to every open tab
    // of this window (private windows never enable dev extras).
    let settings_refresh: Rc<dyn Fn()> = {
        let tabs = tabs.clone();
        let state = state.clone();

        Rc::new(move || {
            let developer_tools = state.settings.borrow().developer_tools && !private_mode;

            for tab in tabs.borrow().iter() {
                if let Some(settings) = webkit6::prelude::WebViewExt::settings(&tab.web_view) {
                    settings.set_enable_developer_extras(developer_tools);
                }
            }
        })
    };

    state.subscribe_settings(&settings_refresh);

    {
        let language_refresh = language_refresh.clone();
        let settings_refresh = settings_refresh.clone();

        let tabs = tabs.clone();

        window.connect_close_request(move |_| {
            let _keep_alive = language_refresh.clone();
            let _keep_alive_settings = settings_refresh.clone();

            // Every WebView signal handler holds `tabs`, and `tabs` holds the
            // WebViews: without this the web processes of a closed window
            // (and any audio they play) would stay alive.
            let closed = std::mem::take(&mut *tabs.borrow_mut());

            for tab in closed {
                tab.web_view.stop_loading();
                tab.web_view.try_close();
            }

            Propagation::Proceed
        });
    }

    super::shortcuts::install_keyboard_shortcuts(
        &window,
        super::shortcuts::ShortcutContext {
            new_tab: new_tab.clone(),
            close_tab: close_tab.clone(),
            reopen_closed_tab: reopen_closed_tab.clone(),
            select_tab: select_tab.clone(),
            tabs: tabs.clone(),
            active_id: active_id.clone(),
            toolbar_slot: toolbar_slot.clone(),
            current_web_view: current_web_view.clone(),
            go_back: go_back.clone(),
            go_forward: go_forward.clone(),
            navigate_tab: navigate_tab.clone(),
            app: app.clone(),
            state: state.clone(),
            private_mode,
            window_weak: window_weak.clone(),
            findbar_slot: findbar_slot.clone(),
            bookmark_bar_slot: bookmark_bar_slot.clone(),
        },
    );

    register_window_actions(
        &window,
        WindowActions {
            new_tab: new_tab.clone(),
            reopen_closed_tab: reopen_closed_tab.clone(),
            open_window: {
                let app = app.clone();
                let state = state.clone();

                Rc::new(move || {
                    build_window(&app, state.clone());
                })
            },
            open_private_toggle: {
                let app = app.clone();
                let state = state.clone();
                let window_weak = window_weak.clone();

                Rc::new(move || {
                    // Toggle: from a private window this opens a normal
                    // window instead.
                    if private_mode {
                        build_window(&app, state.clone());

                        // "Exit private mode" really leaves it: close this
                        // window so no private session is left open behind
                        // the new one.
                        if let Some(window) = window_weak.upgrade() {
                            window.close();
                        }
                    } else {
                        build_private_window(&app, state.clone());
                    }
                })
            },
        },
    );

    let first_tab = new_tab(None);

    active_id.set(first_tab);

    window.present();

    maximize_window(&window);
}
