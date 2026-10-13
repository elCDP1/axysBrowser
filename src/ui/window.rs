use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box, EventControllerKey, Orientation, Stack,
    StackTransitionType, gdk, gio, glib::Propagation, style_context_add_provider_for_display,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use webkit6::{LoadEvent, NetworkSession, WebView, prelude::WebViewExt};

type NewTabCallback = Rc<dyn Fn(Option<&str>) -> usize>;

use crate::app_state::AppState;
use crate::browser::address::{SearchConfig, resolve_input};
use crate::browser::engine::BrowserEngine;
use crate::browser::tabs::Tab;
use crate::internal::router;

use super::bookmarks::BookmarkBar;
use super::tabs::TabBar;
use super::toolbar::Toolbar;

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
        "Privacy mode"
    } else {
        "axysBrowser"
    };

    let window = ApplicationWindow::builder()
        .application(app)
        .title(window_title)
        .icon_name("axysbrowser")
        .resizable(true)
        .build();

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

    if private_mode {
        let banner = gtk::Label::new(Some(&format!(
            "{} · temporary website data · {}",
            rust_i18n::t!("privacy.title"),
            rust_i18n::t!("privacy.search"),
        )));

        banner.add_css_class("private-banner");

        banner.set_margin_top(3);
        banner.set_margin_bottom(2);
        banner.set_margin_start(8);
        banner.set_margin_end(8);

        root.append(&banner);
    }

    let tabs = Rc::new(RefCell::new(Vec::<Tab>::new()));

    let active_id = Rc::new(Cell::new(usize::MAX));
    let next_id = Rc::new(Cell::new(0usize));

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

    let select_tab: Rc<dyn Fn(usize)> = {
        let tabs = tabs.clone();
        let active_id = active_id.clone();
        let top_stack = top_stack.clone();
        let current_web_view = current_web_view.clone();
        let toolbar_slot = toolbar_slot.clone();
        let tabbar_slot = tabbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();

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

            *current_web_view.borrow_mut() = Some(web_view.clone());

            if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                toolbar.address.set_text(&uri);

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

                // Don't leave a heavy page loading in a hidden view.
                web_view.stop_loading();

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
                        toolbar.address.set_text(&target);

                        toolbar.set_loading(false);

                        toolbar.set_navigation_state(can_go_back, can_go_forward);
                    }

                    if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                        bookmark_bar.refresh();
                    }
                }

                return;
            }

            if !target.starts_with("http://") && !target.starts_with("https://") {
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
                    toolbar.address.set_text(&target);

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
                // Best-effort sync of the lightweight custom history so the
                // address bar / tab state stays coherent.
                if let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id) {
                    let _ = tab.go_back();
                }

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

            if router::page_name(&target).is_some()
                && let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id)
            {
                tab.title = router::page_title(&target);
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
                    toolbar.address.set_text(&target);

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
                if let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id) {
                    let _ = tab.go_forward();
                }

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

            if router::page_name(&target).is_some()
                && let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id)
            {
                tab.title = router::page_title(&target);
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
                    toolbar.address.set_text(&target);

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
            }
        })
    };

    // Popup opener with late binding: `new_tab` needs it at creation time,
    // but it needs `new_tab` to open the URL. Resolved via slot + idle to
    // also avoid re-entrancy inside WebKit's `create` signal.
    let new_tab_slot: Rc<RefCell<Option<NewTabCallback>>> = Rc::new(RefCell::new(None));

    let open_in_new_tab: Rc<dyn Fn(String)> = {
        let new_tab_slot = new_tab_slot.clone();

        Rc::new(move |url: String| {
            let new_tab_slot = new_tab_slot.clone();

            gtk::glib::idle_add_local_once(move || {
                if let Some(new_tab) = new_tab_slot.borrow().clone() {
                    new_tab(Some(&url));
                }
            });
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

            BrowserEngine::configure(&web_view, state.settings.borrow().developer_tools);
            BrowserEngine::wire_view(&web_view, open_in_new_tab.clone());

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

            let local_extensions_changed = {
                let toolbar_slot = toolbar_slot.clone();

                Rc::new(move |visible: bool| {
                    if let Some(slot) = toolbar_slot.upgrade()
                        && let Some(toolbar) = slot.borrow().as_ref()
                    {
                        toolbar.set_extensions_visible(visible);
                    }
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
                    local_extensions_changed.clone(),
                    local_downloads.clone(),
                    local_history.clone(),
                    local_clear_browsing_data.clone(),
                    state.clone(),
                ) && let Some(name) = router::page_name(uri)
                {
                    content.add_named(&widget, Some(name));
                }
            }

            let first_uri = starting_uri.map(|uri| uri.to_string()).unwrap_or_else(|| {
                if private_mode {
                    "axys://privacy".to_string()
                } else {
                    "axys://newtab".to_string()
                }
            });

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
                toolbar.address.set_text(&first_uri);

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
                        toolbar.address.set_text(&uri);
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
                let active_id = active_id.clone();
                let toolbar_slot = toolbar_slot.clone();
                let state = state.clone();

                web_view.connect_load_changed(move |view, event| {
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

                                if !private_mode
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
                        BrowserEngine::reload(view);
                    }
                }
            })
        },
        go_back.clone(),
        go_forward.clone(),
        state.clone(),
    );

    *toolbar_slot.borrow_mut() = Some(toolbar);

    let bookmark_bar = BookmarkBar::new(state.clone(), {
        let navigate_tab = navigate_tab.clone();
        let active_id = active_id.clone();

        Rc::new(move |url: String| {
            navigate_tab(active_id.get(), url);
        })
    });

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

    root.append(&top_stack);

    window.set_child(Some(&root));

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

                let local_extensions_changed = {
                    let toolbar_slot = toolbar_slot.clone();

                    Rc::new(move |visible: bool| {
                        if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                            toolbar.set_extensions_visible(visible);
                        }
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
                        local_extensions_changed.clone(),
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
                    toolbar.address.set_text(&tab.uri);
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

    // Live-apply developer tools and the extensions button to every open tab
    // of this window (private windows never enable dev extras).
    let settings_refresh: Rc<dyn Fn()> = {
        let tabs = tabs.clone();
        let toolbar_slot = toolbar_slot.clone();
        let state = state.clone();

        Rc::new(move || {
            let developer_tools = state.settings.borrow().developer_tools && !private_mode;
            let show_extensions = state.settings.borrow().show_extensions;

            for tab in tabs.borrow().iter() {
                if let Some(settings) = webkit6::prelude::WebViewExt::settings(&tab.web_view) {
                    settings.set_enable_developer_extras(developer_tools);
                }
            }

            if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                toolbar.set_extensions_visible(show_extensions);
            }
        })
    };

    state.subscribe_settings(&settings_refresh);

    {
        let language_refresh = language_refresh.clone();
        let settings_refresh = settings_refresh.clone();

        window.connect_close_request(move |_| {
            let _keep_alive = language_refresh.clone();
            let _keep_alive_settings = settings_refresh.clone();

            Propagation::Proceed
        });
    }

    let keyboard = EventControllerKey::new();

    {
        let new_tab = new_tab.clone();
        let close_tab = close_tab.clone();
        let select_tab = select_tab.clone();
        let tabs = tabs.clone();
        let active_id = active_id.clone();
        let toolbar_slot = toolbar_slot.clone();
        let current_web_view = current_web_view.clone();
        let go_back = go_back.clone();
        let go_forward = go_forward.clone();
        let app_for_shortcuts = app.clone();
        let state_for_shortcuts = state.clone();

        keyboard.connect_key_pressed(move |_, key, _, modifiers| {
            let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);

            let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);

            let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);

            if ctrl {
                if let Some(ch) = key.to_unicode() {
                    match ch.to_ascii_lowercase() {
                        'n' => {
                            if shift {
                                build_private_window(
                                    &app_for_shortcuts,
                                    state_for_shortcuts.clone(),
                                );
                            } else {
                                build_window(&app_for_shortcuts, state_for_shortcuts.clone());
                            }

                            return Propagation::Stop;
                        }

                        't' => {
                            new_tab(None);

                            return Propagation::Stop;
                        }

                        'w' => {
                            close_tab(active_id.get());

                            return Propagation::Stop;
                        }

                        'l' => {
                            if let Some(toolbar) = toolbar_slot.borrow().as_ref() {
                                toolbar.address.grab_focus();

                                if toolbar.address.text().starts_with("axys://") {
                                    toolbar.address.set_text("");
                                }

                                toolbar.address.select_region(0, -1);
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

            if key == gdk::Key::Escape
                && let Some(view) = current_web_view.borrow().as_ref()
                && view.is_loading()
            {
                view.stop_loading();

                return Propagation::Stop;
            }

            Propagation::Proceed
        });

        window.add_controller(keyboard);
    }

    {
        let new_tab = new_tab.clone();

        let action = gio::SimpleAction::new("new-tab", None);

        action.connect_activate(move |_, _| {
            new_tab(None);
        });

        window.add_action(&action);
    }

    {
        let app = app.clone();
        let state = state.clone();

        let action = gio::SimpleAction::new("new-window", None);

        action.connect_activate(move |_, _| {
            build_window(&app, state.clone());
        });

        window.add_action(&action);
    }

    {
        let app = app.clone();
        let state = state.clone();

        let action = gio::SimpleAction::new("privacy", None);

        action.connect_activate(move |_, _| {
            build_private_window(&app, state.clone());
        });

        window.add_action(&action);
    }

    {
        let new_tab = new_tab.clone();

        let action = gio::SimpleAction::new("tools", None);

        action.connect_activate(move |_, _| {
            new_tab(Some("axys://tools"));
        });

        window.add_action(&action);
    }

    {
        let new_tab = new_tab.clone();

        let action = gio::SimpleAction::new("downloads", None);

        action.connect_activate(move |_, _| {
            new_tab(Some("axys://downloads"));
        });

        window.add_action(&action);
    }

    {
        let new_tab = new_tab.clone();

        let action = gio::SimpleAction::new("history", None);

        action.connect_activate(move |_, _| {
            new_tab(Some("axys://history"));
        });

        window.add_action(&action);
    }

    {
        let new_tab = new_tab.clone();

        let action = gio::SimpleAction::new("settings", None);

        action.connect_activate(move |_, _| {
            new_tab(Some("axys://settings"));
        });

        window.add_action(&action);
    }

    let first_tab = new_tab(None);

    active_id.set(first_tab);

    window.present();

    maximize_window(&window);
}
