use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box, Orientation, Stack, StackTransitionType, gdk, gio,
    glib::Propagation, style_context_add_provider_for_display,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use webkit6::{ContextMenuItem, LoadEvent, NetworkSession, WebView, prelude::WebViewExt};

use crate::app_state::AppState;
use crate::browser::address::{SearchConfig, resolve_input};
use crate::browser::engine::BrowserEngine;
use crate::browser::tabs::{NewTabCallback, Tab};
use crate::internal::pages::resources::TabRow;
use crate::internal::pages::tools::ToolAction;
use crate::internal::router;
use crate::internal::router::PageCallbacks;

use super::actions::{WindowActions, register_window_actions};
use super::bookmarks::BookmarkBar;
use super::findbar::FindBar;
use super::navigation::{address_text_for, begin_web_load};
use super::tabs::TabBar;
use super::toolbar::{Toolbar, ToolbarOptions};

fn maximize_window(window: &ApplicationWindow) {
    window.maximize();
}

/// Process-wide counter for web-notification ids. Per-window counters reused
/// the same ids across windows, so dismissing one notification also wiped
/// every other window's notification with that id.
static NEXT_WEB_NOTIFICATION: AtomicU64 = AtomicU64::new(1);

/// Schemes whose loaded page makes a tab a real "web" page (as opposed to
/// an internal `axys://` page). Used to decide what the page-scoped tools
/// (view source, inspector, zoom) should act on.
fn is_web_uri(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://") || uri.starts_with("file://")
}

pub fn build_window(app: &Application, state: Rc<AppState>) {
    build_window_with_url(app, state, None);
}

/// Opens a normal window, navigating its first tab to `url` when given.
/// Used for URLs handed by the system (default-browser launches,
/// `xdg-open`), which plain activation would otherwise drop.
pub fn build_window_with_url(app: &Application, state: Rc<AppState>, url: Option<String>) {
    let session = state.network_session.clone();
    let search = state.search.clone();

    build_browser_window(
        app,
        state.clone(),
        session,
        search,
        false,
        state.downloads.clone(),
        url,
    );
}

pub fn build_private_window(app: &Application, state: Rc<AppState>) {
    let session = BrowserEngine::create_ephemeral_session();

    // Private downloads get their own manager: they never appear in normal
    // windows and die with this window. Desktop toasts are muted so
    // filenames don't linger in the shell.
    let downloads = crate::browser::downloads::DownloadManager::new(app.clone());
    downloads.set_private();
    downloads.watch(&session);

    // The search engine follows the profile setting (Chromium-like). Private
    // windows used to force Brave on a private copy, which then silently
    // ignored any engine the user picked while the window was open.
    let search = state.search.clone();

    build_browser_window(app, state, session, search, true, downloads, None);
}

fn build_browser_window(
    app: &Application,
    state: Rc<AppState>,
    session: NetworkSession,
    search: Rc<RefCell<SearchConfig>>,
    private_mode: bool,
    downloads: crate::browser::downloads::DownloadManager,
    initial_url: Option<String>,
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

    // Per-tab factories that build an `axys://` page on first use. Pages are
    // expensive (history/bookmark rows, favicon loads, /proc scans), so they
    // are no longer prebuilt for every tab: only the page a tab actually
    // shows is constructed. Entries are dropped when the tab closes.
    type PageBuilder = Rc<dyn Fn(&str) -> Option<gtk::Widget>>;

    let page_builders: Rc<RefCell<HashMap<usize, PageBuilder>>> =
        Rc::new(RefCell::new(HashMap::new()));

    // One permission manager per window: private windows share a single
    // ephemeral one across all their tabs (never persisted), normal
    // windows share the profile-wide persisted one.
    let window_permissions = if private_mode {
        crate::browser::permissions::PermissionManager::ephemeral()
    } else {
        state.permissions.clone()
    };

    // One history store per window: private windows get an ephemeral one so
    // the session's visits never touch disk *and* their `axys://history`
    // page cannot expose the profile's real, persisted history.
    let window_history = if private_mode {
        crate::browser::history::HistoryManager::ephemeral()
    } else {
        state.history.clone()
    };

    // "Clear browsing data" must act on *this window's* session/history: a
    // private window has to clear its own ephemeral data and must never wipe
    // the profile's cookies, website data, history or on-disk cache.
    let window_clear: crate::app_state::ClearBrowsingData = {
        let state = state.clone();
        let session = session.clone();
        let window_history = window_history.clone();
        let wipe_disk_cache = !private_mode;

        Rc::new(move |done| {
            state.clear_browsing_data(&session, &window_history, wipe_disk_cache, Some(done));
        })
    };

    let top_stack = Stack::new();

    top_stack.set_hexpand(true);
    top_stack.set_vexpand(true);
    top_stack.set_halign(gtk::Align::Fill);
    top_stack.set_valign(gtk::Align::Fill);
    top_stack.set_transition_type(StackTransitionType::None);

    let current_web_view = Rc::new(RefCell::new(None::<WebView>));

    // Last web (non-internal) view that committed a load anywhere in this
    // window, held weakly so a closed tab's WebView can still be freed.
    // The page-scoped tools fall back to it when the active tab is showing
    // an internal page (e.g. the Tools page itself).
    let last_web_view: Rc<RefCell<Option<gtk::glib::WeakRef<WebView>>>> =
        Rc::new(RefCell::new(None));

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
            }) || bookmark_bar_slot
                .borrow()
                .as_ref()
                .is_some_and(|bar| bar.overflow_panel.is_visible());

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

    // Late-bound slot for `new_tab` (filled once it exists); shared with
    // close_tab (replaces a closed last tab) and the popup opener.
    let new_tab_slot: Rc<RefCell<Option<NewTabCallback>>> = Rc::new(RefCell::new(None));

    let close_tab: Rc<dyn Fn(usize)> = {
        let new_tab_slot = Rc::downgrade(&new_tab_slot);
        let tabs = tabs.clone();
        let active_id = active_id.clone();
        let top_stack = top_stack.clone();
        let tabbar_slot = tabbar_slot.clone();
        let select_tab = select_tab.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();
        let closed_tabs = closed_tabs.clone();
        let findbar_slot = findbar_slot.clone();
        let page_builders = page_builders.clone();

        Rc::new(move |id| {
            // Closing the last tab: open a fresh blank tab first and then
            // close the old one through the normal path below. Reusing the
            // old WebView (load_html("")) kept its web process, page cache
            // and back/forward list — i.e. the closed page's memory and
            // history — alive; a new view lets WebKit release all of it.
            if tabs.borrow().len() == 1 {
                if !tabs.borrow().iter().any(|tab| tab.id == id) {
                    return;
                }

                let Some(new_tab) = new_tab_slot
                    .upgrade()
                    .and_then(|slot| slot.borrow().clone())
                else {
                    return;
                };

                new_tab(None);
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

            page_builders.borrow_mut().remove(&id);

            top_stack.remove(&removed.content);

            removed.web_view.stop_loading();

            // Each tab owns its web process (views are never "related"), so
            // end it explicitly: otherwise WebKit may keep the closed page's
            // process, and the page's memory with it, alive in its process
            // cache. TerminatedByApi is ignored by the crash handler.
            removed.web_view.terminate_web_process();

            // Drop find hooks so the closed tab's controller (and view)
            // can be freed; they re-hook lazily on the next search.
            if let Some(findbar) = findbar_slot.borrow().as_ref() {
                findbar.forget_hook(&removed.web_view);
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

    // Page-scoped tools (view source, zoom) work on the active tab when it
    // holds a real page, and otherwise on the last web page that loaded in
    // this window. The inspector is WebKit's own right-click entry.
    let target_view: Rc<dyn Fn() -> Option<WebView>> = {
        let current_web_view = current_web_view.clone();
        let last_web_view = last_web_view.clone();

        Rc::new(move || {
            if let Some(view) = current_web_view.borrow().as_ref()
                && view.uri().map(|uri| is_web_uri(&uri)).unwrap_or(false)
            {
                return Some(view.clone());
            }

            last_web_view
                .borrow()
                .as_ref()
                .and_then(|weak| weak.upgrade())
        })
    };

    let tab_rows: Rc<dyn Fn() -> Vec<TabRow>> = {
        // Weak: `tabs` reaches every tab's content Stack, which is the parent
        // of the pages that store this callback. A strong ref would be a
        // `page -> tab_rows -> tabs -> content -> page` cycle.
        let tabs = Rc::downgrade(&tabs);
        let active_id = active_id.clone();

        Rc::new(move || {
            let Some(tabs) = tabs.upgrade() else {
                return Vec::new();
            };

            tabs.borrow()
                .iter()
                .map(|tab| TabRow {
                    id: tab.id,
                    title: tab.title.clone(),
                    url: tab.uri.clone(),
                    active: tab.id == active_id.get(),
                })
                .collect()
        })
    };

    let zoom_level: Rc<dyn Fn() -> Option<f64>> = {
        let target_view = target_view.clone();

        Rc::new(move || target_view().map(|view| view.zoom_level()))
    };

    let on_tool: Rc<dyn Fn(ToolAction)> = {
        // Weak: this callback is stored inside every internal page, which is
        // a child of the window. Holding the window strongly would make
        // `page -> on_tool -> window -> page` a cycle that never finalizes.
        let window = window.downgrade();

        Rc::new(move |action| {
            let Some(window) = window.upgrade() else {
                return;
            };

            let name = match action {
                ToolAction::ZoomIn => "win.zoom-in",
                ToolAction::ZoomOut => "win.zoom-out",
                ToolAction::ZoomReset => "win.zoom-reset",
                ToolAction::OpenBookmarks => "win.bookmarks",
                ToolAction::OpenResources => "win.resources",
                ToolAction::OpenDownloadsFolder => "win.open-downloads-folder",
            };

            let _ = gtk::prelude::WidgetExt::activate_action(&window, name, None);
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
        let page_builders = page_builders.clone();

        Rc::new(move |id, input| {
            let target = {
                let config = search.borrow();

                resolve_input(&input, &config)
            };

            // An unknown `axys://` page falls back to New Tab rather than
            // being silently dropped (no feedback at all).
            let target = if target.starts_with("axys://") && router::page_name(&target).is_none() {
                "axys://newtab".to_string()
            } else {
                target
            };

            if let Some(page) = router::page_name(&target) {
                let (content, can_go_back, can_go_forward) = {
                    let mut tabs_ref = tabs.borrow_mut();

                    let Some(tab) = tabs_ref.iter_mut().find(|tab| tab.id == id) else {
                        return;
                    };

                    if tab.uri.starts_with("http://") || tab.uri.starts_with("https://") {
                        let current = tab.uri.clone();

                        tab.push_history(current);
                    }

                    tab.uri = target.clone();

                    tab.title = router::page_title(&target);

                    tab.push_history(target.clone());

                    (tab.content.clone(), tab.can_go_back(), tab.can_go_forward())
                };

                // Switching to an internal page maps it synchronously, and a
                // map handler (axys://resources) reads the tab list. Running this
                // outside the `tabs` borrow avoids a re-entrant borrow panic.
                // The page is built on first use.
                if content.child_by_name(page).is_none() {
                    let builder = page_builders.borrow().get(&id).cloned();

                    if let Some(builder) = builder
                        && let Some(widget) = builder(&target)
                    {
                        content.add_named(&widget, Some(page));
                    }
                }

                content.set_visible_child_name(page);

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
    // Private windows stay silent: title and body would linger in the
    // desktop notification history.
    let show_notification: Rc<dyn Fn(String, String)> = {
        let app = app.clone();

        Rc::new(move |title: String, body: String| {
            if private_mode {
                return;
            }

            let id = NEXT_WEB_NOTIFICATION.fetch_add(1, Ordering::Relaxed);

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
        let last_web_view = last_web_view.clone();

        let tabbar_slot = Rc::downgrade(&tabbar_slot);
        let toolbar_slot = Rc::downgrade(&toolbar_slot);
        let bookmark_bar_slot = Rc::downgrade(&bookmark_bar_slot);

        // Weak: pages keep these callbacks alive, and they reach `top_stack`
        // /the window, so strong captures here would leak every closed
        // window's WebViews.
        let navigate_tab = Rc::downgrade(&navigate_tab);
        let open_in_new_tab = open_in_new_tab.clone();
        let show_notification = show_notification.clone();
        let state = state.clone();
        let session = session.clone();
        let downloads = downloads.clone();
        let close_tab = Rc::downgrade(&close_tab);
        let window_history = window_history.clone();
        let window_clear = window_clear.clone();
        let tab_rows = tab_rows.clone();
        let on_tool = on_tool.clone();
        let zoom_level = zoom_level.clone();
        let page_builders = page_builders.clone();

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

            let permissions = window_permissions.clone();

            let on_authenticate: crate::browser::engine::AuthenticateHandler =
                Rc::new(crate::ui::auth::prompt);

            BrowserEngine::wire_view(
                &web_view,
                open_in_new_tab.clone(),
                show_notification.clone(),
                on_authenticate,
                permissions,
            );

            // WebKit's default menu already contains the usual navigation,
            // link/image and "Inspect Element" entries (the latter when
            // developer extras are enabled). Append the Chromium-style
            // "View page source" entry instead of replacing that menu.
            {
                let web_view_weak = web_view.downgrade();

                web_view.connect_context_menu(move |_, menu, _hit_test| {
                    let Some(web_view) = web_view_weak.upgrade() else {
                        return false;
                    };

                    let action = gio::SimpleAction::new("view-source", None);

                    {
                        let web_view = web_view.downgrade();

                        action.connect_activate(move |_, _| {
                            if let Some(web_view) = web_view.upgrade() {
                                let _ = web_view.activate_action("win.view-source", None);
                            }
                        });
                    }

                    menu.append(&ContextMenuItem::new_separator());
                    menu.append(&ContextMenuItem::from_gaction(
                        &action,
                        &rust_i18n::t!("tools.view_source"),
                        None,
                    ));

                    // false: let WebKit show its default menu with our item.
                    false
                });
            }

            if private_mode
                && let Some(settings) = webkit6::prelude::WebViewExt::settings(&web_view)
            {
                settings.set_enable_developer_extras(false);
            }

            content.add_named(&web_view, Some("web"));

            // Wrapped once: pages store these, so they must not hold
            // `navigate_tab` (-> `top_stack`) or `close_tab` strongly.
            let local_navigate_tab: Rc<dyn Fn(usize, String)> = {
                let navigate_tab = navigate_tab.clone();

                Rc::new(move |id, input| {
                    if let Some(navigate_tab) = navigate_tab.upgrade() {
                        navigate_tab(id, input);
                    }
                })
            };

            let local_close_tab: Rc<dyn Fn(usize)> = {
                let close_tab = close_tab.clone();

                Rc::new(move |id| {
                    if let Some(close_tab) = close_tab.upgrade() {
                        close_tab(id);
                    }
                })
            };

            let local_search = {
                let navigate_tab = local_navigate_tab.clone();

                Rc::new(move |input: String| {
                    navigate_tab(id, input);
                })
            };

            let local_about = {
                let navigate_tab = local_navigate_tab.clone();

                Rc::new(move || {
                    navigate_tab(id, "axys://about".to_string());
                })
            };

            let local_downloads = {
                let navigate_tab = local_navigate_tab.clone();

                Rc::new(move || {
                    navigate_tab(id, "axys://downloads".to_string());
                })
            };

            let local_history = {
                let navigate_tab = local_navigate_tab.clone();

                Rc::new(move || {
                    navigate_tab(id, "axys://history".to_string());
                })
            };

            let local_clear_browsing_data = window_clear.clone();

            let build_page: PageBuilder = {
                let local_search = local_search.clone();
                let local_about = local_about.clone();
                let local_downloads = local_downloads.clone();
                let local_history = local_history.clone();
                let local_clear_browsing_data = local_clear_browsing_data.clone();
                let local_close_tab = local_close_tab.clone();
                let window_history = window_history.clone();
                let tab_rows = tab_rows.clone();
                let on_tool = on_tool.clone();
                let zoom_level = zoom_level.clone();
                let state = state.clone();
                let downloads = downloads.clone();

                Rc::new(move |uri: &str| {
                    router::route(
                        uri,
                        PageCallbacks {
                            on_search: local_search.clone(),
                            on_about: local_about.clone(),
                            on_downloads: local_downloads.clone(),
                            on_history: local_history.clone(),
                            on_clear_browsing_data: local_clear_browsing_data.clone(),
                            history: window_history.clone(),
                            tab_rows: tab_rows.clone(),
                            on_close_tab: local_close_tab.clone(),
                            on_tool: on_tool.clone(),
                            zoom_level: zoom_level.clone(),
                        },
                        state.clone(),
                        downloads.clone(),
                    )
                })
            };

            page_builders.borrow_mut().insert(id, build_page.clone());

            // Private windows open a normal new tab too; private mode is
            // indicated by the window title and menu, not a landing page.
            let first_uri = starting_uri
                .map(|uri| uri.to_string())
                .unwrap_or_else(|| "axys://newtab".to_string());

            // A non-internal starting URI (popup, `window.open`, reopened
            // tab) must actually load in the web child; otherwise the tab
            // would sit on the blank New Tab page and never navigate.
            let internal_page = router::page_name(&first_uri);

            let first_page = internal_page.unwrap_or("web");

            // Only the initial internal page is built now; the rest are
            // constructed lazily by `navigate_tab` on first visit.
            if let Some(page) = internal_page
                && let Some(widget) = build_page(&first_uri)
            {
                content.add_named(&widget, Some(page));
            }

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

            if internal_page.is_none() {
                BrowserEngine::load(&web_view, &first_uri);

                begin_web_load(&tabs, id, &web_view, &first_uri);
            }

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
                    // Collapse whitespace: page titles can contain newlines
                    // or control runs that would blow up the tab button.
                    let title = view
                        .title()
                        .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
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
                let last_web_view = last_web_view.clone();

                web_view.connect_load_changed(move |view, event| {
                    if event == LoadEvent::Committed {
                        if let Ok(mut tabs) = tabs.try_borrow_mut()
                            && let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id)
                        {
                            tab.committed = true;
                        }

                        if view.uri().map(|uri| is_web_uri(&uri)).unwrap_or(false) {
                            *last_web_view.borrow_mut() = Some(view.downgrade());
                        }
                    }
                });
            }

            {
                let active_id = active_id.clone();
                let toolbar_slot = toolbar_slot.clone();
                let window_history = window_history.clone();

                web_view.connect_load_changed(move |view, event| {
                    // History is per-window, not per-visible-tab: a page
                    // finishing in the background must still be recorded.
                    // Private windows use an ephemeral manager, so this
                    // stays in memory only.
                    if event == LoadEvent::Finished
                        && let Some(uri) = view.uri().map(|uri| uri.to_string())
                        && (uri.starts_with("http://") || uri.starts_with("https://"))
                        && !BrowserEngine::take_error_page(view, &uri)
                    {
                        let title = view
                            .title()
                            .map(|title| title.to_string())
                            .filter(|title| !title.trim().is_empty())
                            .unwrap_or_else(|| uri.clone());

                        window_history.add_visit(title, uri);
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

    // Page-scoped tool callbacks (`win.view-source`, `win.zoom-*`) and the
    // system "open downloads folder" action.
    let view_source: Rc<dyn Fn()> = {
        let target_view = target_view.clone();
        let new_tab = new_tab.clone();
        let tabs = tabs.clone();
        let toolbar_slot = Rc::downgrade(&toolbar_slot);
        let active_id = active_id.clone();

        Rc::new(move || {
            let Some(view) = target_view() else {
                return;
            };

            let base = view.uri().map(|uri| uri.to_string());

            let new_tab = new_tab.clone();
            let tabs = tabs.clone();
            let toolbar_slot = toolbar_slot.clone();
            let active_id = active_id.clone();
            let base_for_load = base.clone();

            view.evaluate_javascript(
                "document.documentElement ? document.documentElement.outerHTML : ''",
                None,
                None,
                gio::Cancellable::NONE,
                move |result| {
                    let source = match result {
                        Ok(value) => value.to_str().to_string(),
                        Err(error) => {
                            eprintln!("Failed to read page source: {error}");

                            return;
                        }
                    };

                    let document = crate::internal::pages::tools::source_document(&source);

                    let id = new_tab(None);

                    let tab_data = tabs
                        .borrow()
                        .iter()
                        .find(|tab| tab.id == id)
                        .map(|tab| (tab.content.clone(), tab.web_view.clone()));

                    if let Some((content, web_view)) = tab_data {
                        // new_tab(None) lands on the New Tab child; switch
                        // to the web child or the source document would be
                        // loaded invisibly.
                        content.set_visible_child_name("web");

                        web_view.load_html(&document, base_for_load.as_deref());
                    }

                    // Mark the tab like Chromium's view-source: address.
                    let label = base_for_load
                        .as_ref()
                        .map(|base| format!("view-source:{base}"))
                        .unwrap_or_else(|| "view-source:".to_string());

                    if let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id) {
                        tab.uri = label.clone();
                        tab.title = rust_i18n::t!("tools.view_source").to_string();
                    }

                    if active_id.get() == id
                        && let Some(slot) = toolbar_slot.upgrade()
                        && let Some(toolbar) = slot.borrow().as_ref()
                    {
                        toolbar.address.set_text(&label);
                    }
                },
            );
        })
    };

    let adjust_zoom: Rc<dyn Fn(f64)> = {
        let target_view = target_view.clone();

        Rc::new(move |delta| {
            if let Some(view) = target_view() {
                let level = if delta == 0.0 {
                    1.0
                } else {
                    (view.zoom_level() + delta).clamp(0.25, 5.0)
                };

                view.set_zoom_level(level);
            }
        })
    };

    let zoom_in = {
        let adjust_zoom = adjust_zoom.clone();

        Rc::new(move || adjust_zoom(0.1))
    };

    let zoom_out = {
        let adjust_zoom = adjust_zoom.clone();

        Rc::new(move || adjust_zoom(-0.1))
    };

    let zoom_reset = {
        let adjust_zoom = adjust_zoom.clone();

        Rc::new(move || adjust_zoom(0.0))
    };

    let open_downloads_folder: Rc<dyn Fn()> = Rc::new(move || {
        let dir = crate::browser::downloads::DownloadManager::downloads_dir();

        let file = gio::File::for_path(&dir);

        let launcher = gtk::FileLauncher::new(Some(&file));

        launcher.launch(None::<&gtk::Window>, gio::Cancellable::NONE, |result| {
            if let Err(error) = result {
                eprintln!("Could not open downloads folder: {error}");
            }
        });
    });

    let reopen_closed_tab: Rc<dyn Fn()> = {
        let closed_tabs = closed_tabs.clone();
        let new_tab = new_tab.clone();

        Rc::new(move || {
            let Some((uri, _title)) = closed_tabs.borrow_mut().pop() else {
                return;
            };

            // `new_tab` renders internal pages *and* loads web URIs, so a
            // single call restores either kind (no double load).
            new_tab(Some(&uri));
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
            downloads: downloads.clone(),
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
        for panel in [&toolbar.menu_panel.container, &toolbar.downloads_panel] {
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

    // Drag and drop onto the page content navigates the active tab
    // (links, files or plain text, like Chromium).
    {
        // Weak: `navigate_tab` reaches `top_stack`, which owns these
        // controllers; a strong capture would leak the whole content tree.
        let nav_urls = Rc::downgrade(&navigate_tab);
        let id_urls = active_id.clone();

        let drop_urls = gtk::DropTarget::new(String::static_type(), gtk::gdk::DragAction::COPY);

        drop_urls.connect_drop(move |_, value, _, _| {
            let Ok(text) = value.get::<String>() else {
                return false;
            };

            if let Some(input) = text.lines().map(str::trim).find(|line| !line.is_empty()) {
                let Some(nav) = nav_urls.upgrade() else {
                    return false;
                };

                nav(id_urls.get(), input.to_string());

                true
            } else {
                false
            }
        });

        top_stack.add_controller(drop_urls);

        let nav_files = Rc::downgrade(&navigate_tab);
        let id_files = active_id.clone();

        let drop_files =
            gtk::DropTarget::new(gtk::gio::File::static_type(), gtk::gdk::DragAction::COPY);

        drop_files.connect_drop(move |_, value, _, _| {
            let Ok(file) = value.get::<gtk::gio::File>() else {
                return false;
            };

            let Some(nav) = nav_files.upgrade() else {
                return false;
            };

            nav(id_files.get(), file.uri().to_string());

            true
        });

        top_stack.add_controller(drop_files);
    }

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
        let findbar_slot = findbar_slot.clone();
        // Weak: the rebuilt internal pages store these callbacks (see new_tab).
        let navigate_tab = Rc::downgrade(&navigate_tab);
        let state = state.clone();
        let window_weak = window_weak.clone();
        let close_tab = Rc::downgrade(&close_tab);
        let window_history = window_history.clone();
        let window_clear = window_clear.clone();
        let tab_rows = tab_rows.clone();
        let on_tool = on_tool.clone();
        let zoom_level = zoom_level.clone();

        Rc::new(move || {
            let tab_data = {
                let tabs_ref = tabs.borrow();

                tabs_ref
                    .iter()
                    .map(|tab| (tab.id, tab.uri.clone(), tab.content.clone()))
                    .collect::<Vec<_>>()
            };

            for (id, uri, content) in tab_data {
                // Rebuild every tab's internal pages: a tab currently on a
                // web page still holds prebuilt `axys://` pages that must
                // pick up the new locale. `current_page` only decides which
                // child stays visible.
                let current_page = router::page_name(&uri);

                let local_navigate_tab: Rc<dyn Fn(usize, String)> = {
                    let navigate_tab = navigate_tab.clone();

                    Rc::new(move |id, input| {
                        if let Some(navigate_tab) = navigate_tab.upgrade() {
                            navigate_tab(id, input);
                        }
                    })
                };

                let local_close_tab: Rc<dyn Fn(usize)> = {
                    let close_tab = close_tab.clone();

                    Rc::new(move |id| {
                        if let Some(close_tab) = close_tab.upgrade() {
                            close_tab(id);
                        }
                    })
                };

                let local_search = {
                    let navigate_tab = local_navigate_tab.clone();

                    Rc::new(move |input: String| {
                        navigate_tab(id, input);
                    })
                };

                let local_about = {
                    let navigate_tab = local_navigate_tab.clone();

                    Rc::new(move || {
                        navigate_tab(id, "axys://about".to_string());
                    })
                };

                let local_downloads = {
                    let navigate_tab = local_navigate_tab.clone();

                    Rc::new(move || {
                        navigate_tab(id, "axys://downloads".to_string());
                    })
                };

                let local_history = {
                    let navigate_tab = local_navigate_tab.clone();

                    Rc::new(move || {
                        navigate_tab(id, "axys://history".to_string());
                    })
                };

                let local_clear_browsing_data = window_clear.clone();

                for page_uri in [
                    "axys://welcome",
                    "axys://newtab",
                    "axys://privacy",
                    "axys://about",
                    "axys://settings",
                    "axys://tools",
                    "axys://downloads",
                    "axys://history",
                    "axys://bookmarks",
                    "axys://resources",
                ] {
                    let Some(page_name) = router::page_name(page_uri) else {
                        continue;
                    };

                    // Pages are built lazily: only rebuild the ones this tab
                    // has actually instantiated (unbuilt ones will pick up the
                    // new locale when first shown).
                    if content.child_by_name(page_name).is_none() {
                        continue;
                    }

                    let Some(widget) = router::route(
                        page_uri,
                        PageCallbacks {
                            on_search: local_search.clone(),
                            on_about: local_about.clone(),
                            on_downloads: local_downloads.clone(),
                            on_history: local_history.clone(),
                            on_clear_browsing_data: local_clear_browsing_data.clone(),
                            history: window_history.clone(),
                            tab_rows: tab_rows.clone(),
                            on_close_tab: local_close_tab.clone(),
                            on_tool: on_tool.clone(),
                            zoom_level: zoom_level.clone(),
                        },
                        state.clone(),
                        downloads.clone(),
                    ) else {
                        continue;
                    };

                    if let Some(old) = content.child_by_name(page_name) {
                        content.remove(&old);
                    }

                    content.add_named(&widget, Some(page_name));
                }

                if let Some(current_page) = current_page {
                    content.set_visible_child_name(current_page);

                    if let Some(tab) = tabs.borrow_mut().iter_mut().find(|tab| tab.id == id) {
                        tab.title = router::page_title(&uri);
                    }
                }
            }

            if let Some(tabbar) = tabbar_slot.borrow().as_ref() {
                tabbar.refresh(&tabs.borrow(), active_id.get());
                tabbar.refresh_labels();
            }

            if let Some(bookmark_bar) = bookmark_bar_slot.borrow().as_ref() {
                bookmark_bar.refresh();
            }

            if let Some(findbar) = findbar_slot.borrow().as_ref() {
                findbar.refresh_labels();
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

    // Live-apply appearance/developer settings to every open tab of this
    // window (private windows never enable dev extras).
    let settings_refresh: Rc<dyn Fn()> = {
        let tabs = tabs.clone();
        let state = state.clone();

        Rc::new(move || {
            let (developer_tools, hardware_acceleration) = {
                let settings = state.settings.borrow();

                (
                    settings.developer_tools && !private_mode,
                    settings.hardware_acceleration,
                )
            };

            for tab in tabs.borrow().iter() {
                if let Some(settings) = webkit6::prelude::WebViewExt::settings(&tab.web_view) {
                    settings.set_enable_developer_extras(developer_tools);
                    BrowserEngine::apply_hardware_acceleration(&settings, hardware_acceleration);
                }
            }
        })
    };

    state.subscribe_settings(&settings_refresh);

    {
        let language_refresh = language_refresh.clone();
        let settings_refresh = settings_refresh.clone();

        let tabs = tabs.clone();

        // Strong owner of the `new_tab` slot. Everything else (popup opener,
        // close_tab) only holds a Weak to it, so without an owner that lives
        // as long as the window the slot died when this function returned
        // and window.open() / target=_blank / OAuth popups could never open
        // a tab. Released on close to break the ownership cycle.
        let new_tab_slot = new_tab_slot.clone();

        // The chrome widgets store callbacks that close over their own
        // slots (TabBar.on_select -> select_tab -> tabbar_slot, Toolbar's
        // panel toggle -> sync_panels -> toolbar_slot, …), forming strong
        // reference cycles. Left alone they keep the whole chrome and every
        // tab's WebView alive after the window closes, so empty them here.
        let toolbar_slot = toolbar_slot.clone();
        let tabbar_slot = tabbar_slot.clone();
        let bookmark_bar_slot = bookmark_bar_slot.clone();
        let findbar_slot = findbar_slot.clone();
        let current_web_view = current_web_view.clone();
        let top_stack = top_stack.clone();

        window.connect_close_request(move |_| {
            let _keep_alive = language_refresh.clone();
            let _keep_alive_settings = settings_refresh.clone();

            new_tab_slot.borrow_mut().take();

            toolbar_slot.borrow_mut().take();
            tabbar_slot.borrow_mut().take();
            bookmark_bar_slot.borrow_mut().take();
            findbar_slot.borrow_mut().take();

            // The pages' `zoom_level` callback reaches the active WebView
            // through this slot; drop it so nothing keeps the last tab alive.
            *current_web_view.borrow_mut() = None;

            // Every WebView signal handler holds `tabs`, and `tabs` holds the
            // WebViews: without this the web processes of a closed window
            // (and any audio they play) would stay alive.
            let closed = std::mem::take(&mut *tabs.borrow_mut());

            for tab in closed {
                // Detach from the stack too: a parent holds a strong ref to
                // its children, and WebKit keeps a view alive while its web
                // process runs, so both edges must go.
                top_stack.remove(&tab.content);

                tab.web_view.stop_loading();
                tab.web_view.try_close();
                tab.web_view.terminate_web_process();
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
            view_source: view_source.clone(),

            zoom_in: zoom_in.clone(),
            zoom_out: zoom_out.clone(),
            zoom_reset: zoom_reset.clone(),
            open_downloads_folder: open_downloads_folder.clone(),
        },
    );

    // Test-only memory self-test (AXYS_MEM_SMOKE=1, used by CI).
    super::memsmoke::start_if_requested(super::memsmoke::SmokeContext {
        tabs: tabs.clone(),
        new_tab: new_tab.clone(),
        close_tab: close_tab.clone(),
        navigate_tab: navigate_tab.clone(),
        close_window: {
            let window = window.clone();

            Rc::new(move || {
                window.close();
            })
        },
        app: app.clone(),
    });

    let first_tab = new_tab(None);

    active_id.set(first_tab);

    // URLs handed by the system land in the first tab (resolved like typed
    // input, so bare domains and searches work too).
    if let Some(url) = initial_url {
        navigate_tab(active_id.get(), url);
    }

    window.present();

    maximize_window(&window);
}
