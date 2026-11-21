//! The seam's page handle, backed by WebKitGTK.
//!
//! Everything the interface needs from a page is here: navigation, zoom,
//! scripting, source, printing, MHTML export, find, and signals as disposable
//! subscriptions. The engine's own types never appear in these signatures, so
//! `ui/` compiles against this surface on both platforms.

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use webkit6::{FindOptions, PrintOperation, SaveMode, WebView, prelude::WebViewExt};

use crate::browser::engine::types::{
    FindOutcome, FindRequest, FindStep, LoadState, MenuItem, Subscription, Unsupported,
};

/// A live page. Cloneable handle; the widget the interface puts in its stack.
#[derive(Clone)]
pub struct View {
    inner: WebView,
    find: Rc<RefCell<FindState>>,
    /// Handlers the page owns. They hold the page weakly and disconnect when
    /// the page goes away, so keeping them here always outlives the signal.
    signals: Rc<RefCell<Vec<Subscription>>>,
}

impl View {
    /// Wraps a view the engine just created.
    pub(crate) fn from_webkit(inner: WebView) -> Self {
        Self {
            inner,
            find: Rc::new(RefCell::new(FindState::default())),
            signals: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// The underlying engine view. Engine-internal only: the interface must
    /// never reach it, which is what the step-8 lint checks.
    pub(crate) fn webkit(&self) -> &WebView {
        &self.inner
    }

    /// The widget to put in the tab's container.
    pub fn widget(&self) -> &gtk::Widget {
        self.inner.upcast_ref()
    }

    /// A handle that does not keep the page alive.
    pub fn downgrade(&self) -> WeakView {
        WeakView::of(&self.inner)
    }

    // Navigation -----------------------------------------------------------

    pub fn load_html(&self, html: &str, base: Option<&str>) {
        self.inner.load_html(html, base);
    }

    /// Reloads ignoring the HTTP cache (Ctrl+Shift+R).
    pub fn reload_ignoring_cache(&self) {
        self.inner.reload_bypass_cache();
    }

    /// The page's icon, for the bookmark dialog's preview.
    pub fn favicon(&self) -> Option<gtk::gdk::Texture> {
        self.inner.favicon()
    }

    /// Whether the page's audio is muted (the tab menu's mute entry).
    pub fn is_muted(&self) -> bool {
        self.inner.is_muted()
    }

    pub fn set_muted(&self, muted: bool) {
        self.inner.set_is_muted(muted);
    }

    pub fn stop_loading(&self) {
        self.inner.stop_loading();
    }

    /// Leaves the page: stops loading, closes the view and ends the process.
    pub fn close(&self) {
        self.inner.stop_loading();
        self.inner.try_close();
        self.inner.terminate_web_process();
    }

    pub fn uri(&self) -> Option<String> {
        self.inner.uri().map(|uri| uri.to_string())
    }

    pub fn title(&self) -> Option<String> {
        self.inner.title().map(|title| title.to_string())
    }

    pub fn is_loading(&self) -> bool {
        self.inner.is_loading()
    }

    pub fn can_go_back(&self) -> bool {
        self.inner.can_go_back()
    }

    pub fn go_back(&self) {
        self.inner.go_back();
    }

    pub fn can_go_forward(&self) -> bool {
        self.inner.can_go_forward()
    }

    pub fn go_forward(&self) {
        self.inner.go_forward();
    }

    // Zoom -----------------------------------------------------------------

    pub fn zoom_level(&self) -> f64 {
        self.inner.zoom_level()
    }

    pub fn set_zoom_level(&self, level: f64) {
        self.inner.set_zoom_level(level);
    }

    // Settings that can change while a page is open -----------------------

    /// Developer extras (the inspector entry). Re-applied when the private
    /// mode rule changes for an already open page.
    pub fn set_developer_extras(&self, enabled: bool) {
        if let Some(settings) = WebViewExt::settings(&self.inner) {
            settings.set_enable_developer_extras(enabled);
        }
    }

    /// The user's GPU preference, applied to a page that is already open.
    pub fn set_hardware_acceleration(&self, enabled: bool) {
        if let Some(settings) = WebViewExt::settings(&self.inner) {
            crate::browser::engine::BrowserEngine::apply_hardware_acceleration(&settings, enabled);
        }
    }

    // Scripting and content ------------------------------------------------

    /// The loaded document's source: the served bytes when they are available,
    /// the DOM's `outerHTML` otherwise.
    pub fn source(&self, done: impl FnOnce(String) + 'static) {
        let pending: SourceSink = Rc::new(RefCell::new(Some(Box::new(done))));

        let from_dom = {
            let pending = pending.clone();
            let inner = self.inner.clone();

            move || {
                let pending = pending.clone();

                inner.evaluate_javascript(
                    "document.documentElement ? document.documentElement.outerHTML : ''",
                    None,
                    None,
                    gio::Cancellable::NONE,
                    move |result| {
                        let text = match result {
                            Ok(value) => value.to_str().to_string(),
                            Err(error) => {
                                eprintln!("Failed to read page source: {error}");

                                String::new()
                            }
                        };

                        if let Some(done) = pending.borrow_mut().take() {
                            done(text);
                        }
                    },
                );
            }
        };

        match self.inner.main_resource() {
            Some(resource) => {
                let pending_fallback = Rc::new(from_dom);

                resource.data(gio::Cancellable::NONE, move |result| match result {
                    Ok(bytes) if !bytes.is_empty() => {
                        if let Some(done) = pending.borrow_mut().take() {
                            done(String::from_utf8_lossy(&bytes).into_owned());
                        }
                    }
                    _ => pending_fallback(),
                });
            }
            None => from_dom(),
        }
    }

    // Page-scoped tools ----------------------------------------------------

    pub fn print(&self, parent: Option<&gtk::ApplicationWindow>) -> Result<(), Unsupported> {
        let operation = PrintOperation::new(&self.inner);

        let _ = operation.run_dialog(parent);

        Ok(())
    }

    pub fn save_mhtml(
        &self,
        file: &gio::File,
        done: impl FnOnce(Result<(), String>) + 'static,
    ) -> Result<(), Unsupported> {
        self.inner.save_to_file(
            file,
            SaveMode::Mhtml,
            gio::Cancellable::NONE,
            move |result| done(result.map_err(|error| error.to_string())),
        );

        Ok(())
    }

    // Find -----------------------------------------------------------------

    /// One call for typing, "next" and "previous". The result always arrives
    /// through the callback, and a newer call supersedes a pending one.
    pub fn find(&self, request: FindRequest, done: impl FnOnce(FindOutcome) + 'static) {
        let Some(controller) = self.inner.find_controller() else {
            done(FindOutcome {
                matches: 0,
                found: false,
            });

            return;
        };

        let mut state = self.find.borrow_mut();

        state.pending = Some((request.query.clone(), Box::new(done)));

        if !state.hooked {
            state.hooked = true;

            let pending = self.find.clone();

            controller.connect_found_text(move |controller, matches| {
                deliver(controller, &pending, matches, matches > 0);
            });

            let pending = self.find.clone();

            controller.connect_failed_to_find_text(move |controller| {
                deliver(controller, &pending, 0, false);
            });
        }

        drop(state);

        let mut options = FindOptions::empty();

        if !request.case_sensitive {
            options |= FindOptions::CASE_INSENSITIVE;
        }

        if request.wrap {
            options |= FindOptions::WRAP_AROUND;
        }

        match request.step {
            FindStep::First => {
                controller.search(&request.query, options.bits(), request.max_matches)
            }
            FindStep::Next => controller.search_next(),
            FindStep::Previous => controller.search_previous(),
        }
    }

    /// Ends a search: clears the highlights and any result still in flight.
    pub fn find_stop(&self) {
        self.find.borrow_mut().pending = None;

        if let Some(controller) = self.inner.find_controller() {
            controller.search_finish();
        }
    }

    // Signals --------------------------------------------------------------

    pub fn on_load(&self, f: impl Fn(LoadState) + 'static) {
        let id = self.inner.connect_load_changed(move |_, event| {
            if let Some(state) = load_state(event) {
                f(state);
            }
        });

        self.track(id)
    }

    pub fn on_uri_changed(&self, f: impl Fn(Option<&str>) + 'static) {
        let id = self.inner.connect_uri_notify(move |view| {
            f(view.uri().as_deref());
        });

        self.track(id)
    }

    pub fn on_title_changed(&self, f: impl Fn(Option<&str>) + 'static) {
        let id = self.inner.connect_title_notify(move |view| {
            f(view.title().as_deref());
        });

        self.track(id)
    }

    pub fn on_loading_changed(&self, f: impl Fn(bool) + 'static) {
        let id = self.inner.connect_is_loading_notify(move |view| {
            f(view.is_loading());
        });

        self.track(id)
    }

    /// Asks the interface which extra entries the page's context menu should
    /// carry. The engine's own entries stay; the callback's are appended.
    pub fn on_context_menu(&self, f: impl Fn() -> Vec<MenuItem> + 'static) {
        let weak = self.inner.downgrade();

        let id = self.inner.connect_context_menu(move |_, menu, _hit_test| {
            for item in f() {
                match item {
                    MenuItem::Separator => menu.append(&webkit6::ContextMenuItem::new_separator()),
                    MenuItem::Action { label, action } => {
                        let simple = gio::SimpleAction::new("seam-action", None);

                        {
                            let weak = weak.clone();
                            let action = action.clone();

                            simple.connect_activate(move |_, _| {
                                if let Some(view) = weak.upgrade() {
                                    let _ = view.activate_action(&action, None);
                                }
                            });
                        }

                        menu.append(&webkit6::ContextMenuItem::from_gaction(
                            &simple, &label, None,
                        ));
                    }
                }
            }

            // false: let the engine show its own menu with our entries.
            false
        });

        self.track(id)
    }

    /// Keeps a handler connected for as long as this page is alive.
    fn track(&self, id: glib::SignalHandlerId) {
        let weak = self.inner.downgrade();

        self.signals
            .borrow_mut()
            .push(Subscription::new(Box::new(move || {
                if let Some(view) = weak.upgrade() {
                    view.disconnect(id);
                }
            })));
    }
}

impl Deref for View {
    type Target = gtk::Widget;

    fn deref(&self) -> &Self::Target {
        self.inner.upcast_ref()
    }
}

impl PartialEq for View {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for View {}

/// Delivers a find result only when it belongs to the query it was asked for:
/// `found-text` reports a count, not a request id, so a late reply for an
/// older query must not overwrite a newer search.
fn deliver(
    controller: &webkit6::FindController,
    pending: &Rc<RefCell<FindState>>,
    matches: u32,
    found: bool,
) {
    let mut state = pending.borrow_mut();

    let Some((query, _)) = state.pending.as_ref() else {
        return;
    };

    if controller.search_text().as_deref() != Some(query.as_str()) {
        return;
    }

    if let Some((_, done)) = state.pending.take() {
        drop(state);
        done(FindOutcome { matches, found });
    }
}

fn load_state(event: webkit6::LoadEvent) -> Option<LoadState> {
    match event {
        webkit6::LoadEvent::Started => Some(LoadState::Started),
        webkit6::LoadEvent::Redirected => Some(LoadState::Redirected),
        webkit6::LoadEvent::Committed => Some(LoadState::Committed),
        webkit6::LoadEvent::Finished => Some(LoadState::Finished),
        _ => None,
    }
}

/// Where a page's source is delivered once it has been read.
type SourceSink = Rc<RefCell<Option<Box<dyn FnOnce(String)>>>>;

/// The result a search is waiting for: the query it was asked for and where
/// to send the answer.
type PendingFind = (String, Box<dyn FnOnce(FindOutcome)>);

/// State of a page's find session: at most one result may be outstanding, and
/// the controller's handlers are connected once per page.
#[derive(Default)]
struct FindState {
    pending: Option<PendingFind>,
    hooked: bool,
}

/// A non-owning handle to a page.
#[derive(Clone)]
pub struct WeakView {
    inner: glib::WeakRef<WebView>,
}

impl WeakView {
    /// Wraps an engine view's weak reference, for the places that receive the
    /// engine's own pointer (signal handlers, mostly).
    pub(crate) fn of(view: &WebView) -> Self {
        Self {
            inner: view.downgrade(),
        }
    }

    /// The page, if it is still alive.
    pub fn upgrade(&self) -> Option<View> {
        self.inner.upgrade().map(View::from_webkit)
    }
}
