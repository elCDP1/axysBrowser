//! The seam's page handle, backed by Servo.
//!
//! Servo renders into a `RenderingContext`, not into a widget, so this file is
//! the bridge: one `gtk::Picture` shows the last frame, a delegate turns the
//! engine's notifications into the seam's callbacks, and GTK input events are
//! translated to Servo's `InputEvent`s.
//!
//! Rendering is software: `SoftwareRenderingContext` → `read_to_image` →
//! `gdk::MemoryTexture` → the picture. That is the route the port's plan picks
//! for Windows (§8 of `docs/porting/phase4-engine-api.md`) because it needs no
//! native handle, so the same code works on a GPU-less machine and under a
//! remote session. The cost is one readback and one texture upload per painted
//! frame; the pump only pays it when Servo reported a new frame.
//!
//! The engine is pumped by [`super`], not by this file: a 16 ms timeout spins
//! Servo's event loop and then paints the pages that asked for it.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use servo::{
    AuthenticationRequest, Code, ContextMenu, ContextMenuAction, ContextMenuItem,
    CreateNewWebViewRequest, DeviceIntPoint, DeviceIntRect, DevicePoint, EmbedderControl,
    InputEvent, JSValue, Key, KeyState, KeyboardEvent, LoadStatus, Location, Modifiers,
    MouseButton, MouseButtonAction, MouseButtonEvent, MouseMoveEvent, NamedKey, NavigationRequest,
    Notification, PermissionRequest, RenderingContext, SimpleDialog, SoftwareRenderingContext,
    WebView, WebViewBuilder, WebViewDelegate, WebViewPoint, WheelDelta, WheelEvent, WheelMode,
};
use url::Url;

use super::BrowserEngine;
use crate::browser::engine::shared;
use crate::browser::engine::types::{
    AuthPrompt, FindOutcome, FindRequest, LoadState, MenuItem, Subscription, Unsupported,
    ViewConfig,
};

/// Size the software context starts at, before the widget's first allocation.
/// A page needs a non-zero surface to exist at all; the first pump resizes it.
const INITIAL_WIDTH: u32 = 1280;
const INITIAL_HEIGHT: u32 = 800;

/// A live page. Cloneable handle; the widget the interface puts in its stack.
#[derive(Clone)]
pub struct View {
    inner: Rc<ViewInner>,
}

impl View {
    /// Creates a page: software context, picture and engine view.
    pub(crate) fn new() -> Self {
        let servo = super::servo();

        let picture = gtk::Picture::new();

        picture.set_content_fit(gtk::ContentFit::Fill);
        picture.set_focusable(true);
        picture.set_hexpand(true);
        picture.set_vexpand(true);

        let scale = picture.scale_factor().max(1) as u32;
        let context: Rc<dyn RenderingContext> = {
            let size = dpi::PhysicalSize::new(INITIAL_WIDTH * scale, INITIAL_HEIGHT * scale);

            let context = SoftwareRenderingContext::new(size).unwrap_or_else(|error| {
                // Without a rendering surface there is no page to show; failing
                // here with the engine's own error is clearer than a tab that
                // silently never draws.
                panic!("Servo could not create a software rendering context: {error:?}");
            });

            if let Err(error) = context.make_current() {
                eprintln!("Servo could not make the software context current: {error:?}");
            }

            Rc::new(context)
        };

        let delegate = Rc::new(Delegate {
            view: RefCell::new(Weak::new()),
        });

        let webview = WebViewBuilder::new(&servo, context.clone())
            .url(Url::parse("about:blank").expect("about:blank is a valid URL"))
            .delegate(delegate.clone())
            .build();

        let inner = Rc::new(ViewInner {
            webview: RefCell::new(Some(webview)),
            picture,
            context,
            dirty: Cell::new(true),
            pointer: Cell::new((0.0, 0.0)),
            size: Cell::new((INITIAL_WIDTH * scale, INITIAL_HEIGHT * scale)),
            muted: Cell::new(false),
            error_page_uri: RefCell::new(None),
            source_view: Cell::new(false),
            hooks: Rc::new(RefCell::new(Hooks::default())),
        });

        *delegate.view.borrow_mut() = Rc::downgrade(&inner);

        let view = Self { inner };

        view.install_input();
        view.install_focus();
        super::register_view(&view.inner);

        view
    }

    /// The underlying backend state. Engine-internal only.
    pub(crate) fn inner(&self) -> &Rc<ViewInner> {
        &self.inner
    }

    /// The widget to put in the tab's container.
    pub fn widget(&self) -> &gtk::Widget {
        self.inner.picture.upcast_ref()
    }

    /// A handle that does not keep the page alive.
    pub fn downgrade(&self) -> WeakView {
        WeakView {
            inner: Rc::downgrade(&self.inner),
        }
    }

    // Navigation -----------------------------------------------------------

    /// Loads a generated document.
    ///
    /// Servo has no `load_html` with a base URL, so the document travels as a
    /// `data:` URL and the base is injected as `<base href>`. That keeps
    /// relative links and images working for the source view.
    pub fn load_html(&self, html: &str, base: Option<&str>) {
        self.inner.load_uri(&shared::data_url(html, base));
    }

    /// Reloads ignoring the HTTP cache (Ctrl+Shift+R).
    ///
    /// Servo has no per-reload cache bypass: the HTTP cache is dropped instead,
    /// which refetches everything the reload touches.
    pub fn reload_ignoring_cache(&self) {
        self.inner.reload_ignoring_cache();
    }

    /// The page's icon.
    ///
    /// Servo exposes the favicon as engine image data, not as a texture; the
    /// conversion is not wired yet, so the bookmark dialog shows its fallback.
    pub fn favicon(&self) -> Option<gdk::Texture> {
        None
    }

    /// Whether the page's audio is muted.
    ///
    /// Servo 0.7 has no per-page mute, so this records the tab menu's choice
    /// without silencing anything (documented gap).
    pub fn is_muted(&self) -> bool {
        self.inner.muted.get()
    }

    pub fn set_muted(&self, muted: bool) {
        self.inner.muted.set(muted);
    }

    pub fn stop_loading(&self) {
        self.inner.stop_loading();
    }

    /// Leaves the page: stops loading, drops the engine view and its surface.
    pub fn close(&self) {
        self.inner.close();
    }

    pub fn uri(&self) -> Option<String> {
        self.inner
            .webview()
            .and_then(|webview| webview.url())
            .map(|url| url.to_string())
    }

    pub fn title(&self) -> Option<String> {
        self.inner
            .webview()
            .and_then(|webview| webview.page_title())
    }

    pub fn is_loading(&self) -> bool {
        self.inner
            .webview()
            .is_some_and(|webview| webview.load_status() != LoadStatus::Complete)
    }

    pub fn can_go_back(&self) -> bool {
        self.inner
            .webview()
            .is_some_and(|webview| webview.can_go_back())
    }

    pub fn go_back(&self) {
        if let Some(webview) = self.inner.webview() {
            webview.go_back(1);
        }
    }

    pub fn can_go_forward(&self) -> bool {
        self.inner
            .webview()
            .is_some_and(|webview| webview.can_go_forward())
    }

    pub fn go_forward(&self) {
        if let Some(webview) = self.inner.webview() {
            webview.go_forward(1);
        }
    }

    // Zoom -----------------------------------------------------------------

    pub fn zoom_level(&self) -> f64 {
        self.inner
            .webview()
            .map(|webview| f64::from(webview.page_zoom()))
            .unwrap_or(1.0)
    }

    pub fn set_zoom_level(&self, level: f64) {
        if let Some(webview) = self.inner.webview() {
            webview.set_page_zoom(level as f32);
        }
    }

    // Settings that can change while a page is open -----------------------

    /// Developer extras (the inspector entry).
    ///
    /// Servo 0.7's embedder API exposes no inspector toggle, so there is
    /// nothing to switch and the context menu has no inspector entry
    /// (documented gap). The setter exists because the interface calls it on a
    /// page that is already open, not because it can do anything.
    pub fn set_developer_extras(&self, _enabled: bool) {}

    /// The user's GPU preference. This backend always renders in software, so
    /// the choice cannot change how a page is drawn.
    pub fn set_hardware_acceleration(&self, _enabled: bool) {}

    // Scripting and content ------------------------------------------------

    /// The loaded document's source.
    ///
    /// WebKit hands over the bytes the server sent when it still has them;
    /// Servo's embedder API has no way to reach the main resource, so this is
    /// always the DOM's `outerHTML`.
    pub fn source(&self, done: impl FnOnce(String) + 'static) {
        let Some(webview) = self.inner.webview() else {
            done(String::new());

            return;
        };

        let pending: SourceSink = Rc::new(RefCell::new(Some(Box::new(done))));

        webview.evaluate_javascript(
            r#"document.documentElement ? document.documentElement.outerHTML : ''"#,
            move |result| {
                let text = match result {
                    Ok(JSValue::String(text)) => text,
                    Ok(_) => String::new(),
                    Err(error) => {
                        // `JavaScriptEvaluationError` has no `Display` impl.
                        eprintln!("Failed to read the page source: {error:?}");

                        String::new()
                    }
                };

                if let Some(done) = pending.borrow_mut().take() {
                    done(text);
                }
            },
        );
    }

    // Page-scoped tools ----------------------------------------------------

    /// Printing needs a print backend Servo 0.7 does not embed.
    pub fn print(&self, _parent: Option<&gtk::ApplicationWindow>) -> Result<(), Unsupported> {
        Err(Unsupported)
    }

    /// MHTML save walks the document and every resource; Servo has no such
    /// serializer.
    pub fn save_mhtml(
        &self,
        _file: &gio::File,
        _done: impl FnOnce(Result<(), String>) + 'static,
    ) -> Result<(), Unsupported> {
        Err(Unsupported)
    }

    // Find -----------------------------------------------------------------

    /// Find in page.
    ///
    /// Servo 0.7 exposes no find API. The seam's signature has no error
    /// channel, so an empty outcome is how "not supported" is reported: the
    /// find bar opens, shows 0, and finds nothing. `find_stop` is the matching
    /// no-op.
    pub fn find(&self, _request: FindRequest, done: impl FnOnce(FindOutcome) + 'static) {
        done(FindOutcome {
            matches: 0,
            found: false,
        });
    }

    pub fn find_stop(&self) {}

    // Signals --------------------------------------------------------------

    pub fn on_load(&self, f: impl Fn(LoadState) + 'static) -> Subscription {
        self.inner
            .store_hook::<dyn Fn(LoadState)>(|hooks| &mut hooks.load, Rc::new(f))
    }

    pub fn on_uri_changed(&self, f: impl Fn(Option<&str>) + 'static) -> Subscription {
        self.inner
            .store_hook::<dyn Fn(Option<&str>)>(|hooks| &mut hooks.uri, Rc::new(f))
    }

    pub fn on_title_changed(&self, f: impl Fn(Option<&str>) + 'static) -> Subscription {
        self.inner
            .store_hook::<dyn Fn(Option<&str>)>(|hooks| &mut hooks.title, Rc::new(f))
    }

    pub fn on_loading_changed(&self, f: impl Fn(bool) + 'static) -> Subscription {
        self.inner
            .store_hook::<dyn Fn(bool)>(|hooks| &mut hooks.loading, Rc::new(f))
    }

    /// Asks the interface which extra entries the page's context menu should
    /// carry. Servo's own entries (back, forward, copy, paste, …) come from the
    /// engine; the callback's are appended.
    pub fn on_context_menu(&self, f: impl Fn() -> Vec<MenuItem> + 'static) -> Subscription {
        self.inner
            .store_hook::<dyn Fn() -> Vec<MenuItem>>(|hooks| &mut hooks.context_menu, Rc::new(f))
    }

    // Input ----------------------------------------------------------------

    fn install_input(&self) {
        let pointer = Rc::downgrade(&self.inner);

        let motion = gtk::EventControllerMotion::new();

        motion.connect_motion(move |_, x, y| {
            if let Some(inner) = pointer.upgrade() {
                inner.pointer.set((x, y));

                let event = InputEvent::MouseMove(MouseMoveEvent::new(inner.point()));

                inner.send(event);
            }
        });
        self.inner.picture.add_controller(motion);

        let pressed = Rc::downgrade(&self.inner);

        let click = gtk::GestureClick::new();

        // 0 = accept every button, not just the primary one.
        click.set_button(0);

        click.connect_pressed(move |gesture, _, x, y| {
            if let Some(inner) = pressed.upgrade() {
                // The page is the focus target: without this, keyboard input
                // keeps going to whatever GTK widget had it before.
                inner.picture.grab_focus();

                inner.pointer.set((x, y));

                if let Some(webview) = inner.webview() {
                    webview.set_focused(true);
                }

                let event = InputEvent::MouseButton(MouseButtonEvent::new(
                    MouseButtonAction::Down,
                    mouse_button(gesture.button()),
                    inner.point(),
                ));

                inner.send(event);
            }
        });

        let released = Rc::downgrade(&self.inner);

        click.connect_released(move |gesture, _, x, y| {
            if let Some(inner) = released.upgrade() {
                inner.pointer.set((x, y));

                let event = InputEvent::MouseButton(MouseButtonEvent::new(
                    MouseButtonAction::Up,
                    mouse_button(gesture.button()),
                    inner.point(),
                ));

                inner.send(event);
            }
        });
        self.inner.picture.add_controller(click);

        let scrolling = Rc::downgrade(&self.inner);

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);

        scroll.connect_scroll(move |_, delta_x, delta_y| {
            if let Some(inner) = scrolling.upgrade() {
                // GTK4's smooth deltas are line-sized on a wheel (±1 per
                // notch), which is what `DeltaLine` means to Servo.
                let event = InputEvent::Wheel(WheelEvent::new(
                    WheelDelta {
                        x: delta_x,
                        y: delta_y,
                        z: 0.0,
                        mode: WheelMode::DeltaLine,
                    },
                    inner.point(),
                ));

                inner.send(event);
            }

            // The window's Ctrl+wheel zoom controller runs in the capture
            // phase and already stopped the event when it handled it.
            glib::Propagation::Proceed
        });
        self.inner.picture.add_controller(scroll);

        let typing = Rc::downgrade(&self.inner);

        let keys = gtk::EventControllerKey::new();

        keys.connect_key_pressed(move |_, keyval, _, state| {
            if let Some(inner) = typing.upgrade()
                && let Some(event) = keyboard_event(KeyState::Down, keyval, state, false)
            {
                inner.send(InputEvent::Keyboard(event));
            }

            glib::Propagation::Proceed
        });

        let typing = Rc::downgrade(&self.inner);

        keys.connect_key_released(move |_, keyval, _, state| {
            if let Some(inner) = typing.upgrade()
                && let Some(event) = keyboard_event(KeyState::Up, keyval, state, false)
            {
                inner.send(InputEvent::Keyboard(event));
            }
        });
        self.inner.picture.add_controller(keys);
    }

    fn install_focus(&self) {
        let entering = Rc::downgrade(&self.inner);
        let focus = gtk::EventControllerFocus::new();

        focus.connect_enter(move |_| {
            if let Some(inner) = entering.upgrade()
                && let Some(webview) = inner.webview()
            {
                webview.set_focused(true);
            }
        });

        let leaving = Rc::downgrade(&self.inner);

        focus.connect_leave(move |_| {
            if let Some(inner) = leaving.upgrade()
                && let Some(webview) = inner.webview()
            {
                webview.set_focused(false);
            }
        });
        self.inner.picture.add_controller(focus);
    }
}

impl Deref for View {
    type Target = gtk::Widget;

    fn deref(&self) -> &Self::Target {
        self.inner.picture.upcast_ref()
    }
}

impl PartialEq for View {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for View {}

/// A non-owning handle to a page.
#[derive(Clone)]
pub struct WeakView {
    inner: Weak<ViewInner>,
}

impl WeakView {
    /// The page, if it is still alive.
    pub fn upgrade(&self) -> Option<View> {
        self.inner.upgrade().map(|inner| View { inner })
    }
}

/// Where a page's source is delivered once it has been read.
type SourceSink = Rc<RefCell<Option<Box<dyn FnOnce(String)>>>>;

/// A callback plus its "disconnected" flag.
///
/// The WebKit backend disconnects real signals. Servo reports everything
/// through one delegate, so disconnecting means this backend stops delivering:
/// the flag is shared with the [`Subscription`] the interface holds, and the
/// emitter checks it before calling.
struct Hook<T: ?Sized> {
    callback: Rc<T>,
    live: Rc<Cell<bool>>,
}

impl<T: ?Sized> Hook<T> {
    fn new(callback: Rc<T>) -> Self {
        Self {
            callback,
            live: Rc::new(Cell::new(true)),
        }
    }

    fn subscription(&self) -> Subscription {
        let live = self.live.clone();

        Subscription::new(Box::new(move || live.set(false)))
    }

    fn get(&self) -> Option<Rc<T>> {
        if self.live.get() {
            Some(self.callback.clone())
        } else {
            None
        }
    }
}

/// A `Hook` around the callback that receives the URI or the title.
type UriHook = Hook<dyn Fn(Option<&str>)>;

/// A `Hook` around the callback that answers an authentication request.
type AuthHook = Hook<dyn Fn(&View, AuthPrompt)>;

/// The authentication handler as the interface hands it over.
type AuthCallback = Rc<dyn Fn(&View, AuthPrompt)>;

/// Everything the interface registered on a page.
#[derive(Default)]
struct Hooks {
    load: Option<Hook<dyn Fn(LoadState)>>,
    uri: Option<UriHook>,
    title: Option<UriHook>,
    loading: Option<Hook<dyn Fn(bool)>>,
    context_menu: Option<Hook<dyn Fn() -> Vec<MenuItem>>>,
    notification: Option<Hook<dyn Fn(String, String)>>,
    authenticate: Option<AuthHook>,
}

/// What a page owns. Backend-internal: the interface only ever holds [`View`].
pub(crate) struct ViewInner {
    webview: RefCell<Option<WebView>>,
    picture: gtk::Picture,
    context: Rc<dyn RenderingContext>,
    /// A frame arrived and has not been shown yet.
    dirty: Cell<bool>,
    /// Last cursor position: Servo's button and wheel events carry their own
    /// point, but a motion event and a click must agree on where the pointer is.
    pointer: Cell<(f64, f64)>,
    /// Last size the engine was told about.
    size: Cell<(u32, u32)>,
    muted: Cell<bool>,
    /// The URI whose failed load is currently displayed as an error page.
    error_page_uri: RefCell<Option<String>>,
    /// The next load is a generated source document, not a visit.
    source_view: Cell<bool>,
    hooks: Rc<RefCell<Hooks>>,
}

impl ViewInner {
    pub(crate) fn webview(&self) -> Option<WebView> {
        self.webview.borrow().clone()
    }

    /// Reads back the last frame and shows it.
    ///
    /// `dirty` is consumed first: a frame that arrives while this runs is
    /// served by the next tick, so no update is lost.
    pub(crate) fn paint_if_dirty(&self) {
        if !self.dirty.replace(false) {
            return;
        }

        let Some(webview) = self.webview() else {
            return;
        };

        self.sync_size(&webview);
        webview.paint();

        let size = self.context.size();
        let rect = DeviceIntRect::new(
            DeviceIntPoint::new(0, 0),
            DeviceIntPoint::new(size.width as i32, size.height as i32),
        );

        // Read before presenting: the software context is double buffered, so
        // after `present` the back buffer is the next, still blank, frame.
        let image = self.context.read_to_image(rect);

        self.context.present();

        let Some(image) = image else {
            return;
        };

        let (width, height) = image.dimensions();

        if width == 0 || height == 0 {
            return;
        }

        let stride = (width as usize) * 4;
        let bytes = glib::Bytes::from_owned(image.into_raw());
        let texture = gdk::MemoryTexture::new(
            width as i32,
            height as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &bytes,
            stride,
        );

        self.picture.set_paintable(Some(&texture));
    }

    /// Keeps the engine's surface the size of the widget.
    fn sync_size(&self, webview: &WebView) {
        let width = self.picture.width().max(1) as u32;
        let height = self.picture.height().max(1) as u32;

        if self.size.get() == (width, height) {
            return;
        }

        self.size.set((width, height));
        webview.resize(dpi::PhysicalSize::new(width, height));
    }

    fn point(&self) -> WebViewPoint {
        let (x, y) = self.pointer.get();

        WebViewPoint::from(DevicePoint::new(x as f32, y as f32))
    }

    fn send(&self, event: InputEvent) {
        if let Some(webview) = self.webview() {
            webview.notify_input_event(event);
        }
    }

    /// Registers a hook and hands back the subscription that disconnects it.
    fn store_hook<T: ?Sized>(
        &self,
        slot: impl FnOnce(&mut Hooks) -> &mut Option<Hook<T>>,
        callback: Rc<T>,
    ) -> Subscription {
        let hook = Hook::new(callback);
        let subscription = hook.subscription();

        *slot(&mut self.hooks.borrow_mut()) = Some(hook);

        subscription
    }

    // Callbacks raised by the delegate ------------------------------------

    fn emit_load(self: &Rc<Self>, state: LoadState) {
        let callback = self
            .hooks
            .borrow()
            .load
            .as_ref()
            .and_then(|hook| hook.get());

        if let Some(callback) = callback {
            callback(state);
        }
    }

    fn emit_uri(self: &Rc<Self>, uri: Option<&str>) {
        let callback = self.hooks.borrow().uri.as_ref().and_then(|hook| hook.get());

        if let Some(callback) = callback {
            callback(uri);
        }
    }

    fn emit_title(self: &Rc<Self>, title: Option<&str>) {
        let callback = self
            .hooks
            .borrow()
            .title
            .as_ref()
            .and_then(|hook| hook.get());

        if let Some(callback) = callback {
            callback(title);
        }
    }

    fn emit_loading(self: &Rc<Self>, loading: bool) {
        let callback = self
            .hooks
            .borrow()
            .loading
            .as_ref()
            .and_then(|hook| hook.get());

        if let Some(callback) = callback {
            callback(loading);
        }
    }

    fn emit_authenticate(self: &Rc<Self>, prompt: AuthPrompt) {
        let callback = self
            .hooks
            .borrow()
            .authenticate
            .as_ref()
            .and_then(|hook| hook.get());

        // With no prompt registered the request is dropped, which cancels it:
        // exactly what the seam promises for a request nobody claims.
        if let Some(callback) = callback {
            callback(
                &View {
                    inner: self.clone(),
                },
                prompt,
            );
        }
    }

    // Navigation -----------------------------------------------------------

    pub(crate) fn load_uri(&self, uri: &str) {
        let Some(webview) = self.webview() else {
            return;
        };

        match Url::parse(uri) {
            Ok(url) => webview.load(url),
            Err(error) => eprintln!(
                "Refusing to load {}: {error}",
                crate::browser::util::log_safe_uri(uri)
            ),
        }
    }

    fn reload_ignoring_cache(&self) {
        let Some(webview) = self.webview() else {
            return;
        };

        super::servo().network_manager().clear_cache();
        webview.reload();
    }

    fn stop_loading(&self) {
        if let Some(webview) = self.webview() {
            // Servo 0.7 has no embedder-level stop; `window.stop()` is the
            // document's own way to abandon the load, and it is what a page
            // sees when the user presses Escape.
            webview.evaluate_javascript("window.stop()", |_| {});
        }
    }

    fn close(&self) {
        // Dropping the engine view is what releases the page, its surface and
        // its delegate; the pump prunes the dead weak reference.
        if let Some(webview) = self.webview.borrow_mut().take() {
            webview.set_focused(false);
        }
    }

    // Failure and generated pages -----------------------------------------

    /// Replaces the document with the load-failure page, keeping the URL.
    ///
    /// Unlike a navigation to a generated page, `document.write` leaves the
    /// address bar showing the URL that failed, which is what the user needs to
    /// see (and what the history check expects).
    fn show_error_page(&self, uri: &str, title: &str, detail: &str) {
        *self.error_page_uri.borrow_mut() = Some(uri.to_string());

        let Some(webview) = self.webview() else {
            return;
        };

        let html = shared::error_page(title, detail, uri);
        let script = format!(
            "document.open(); document.write({}); document.close();",
            shared::js_string_literal(&html)
        );

        webview.evaluate_javascript(script, |result| {
            if let Err(error) = result {
                eprintln!("Could not show the load-failure page: {error:?}");
            }
        });
    }

    fn take_error_page(&self, uri: &str) -> bool {
        let mut marked = self.error_page_uri.borrow_mut();
        let matched = marked.as_deref() == Some(uri);

        if matched {
            *marked = None;
        }

        matched
    }

    fn mark_source_view(&self) {
        self.source_view.set(true);
    }

    fn take_source_view(&self) -> bool {
        self.source_view.replace(false)
    }

    // Context menu ---------------------------------------------------------

    fn show_context_menu(self: &Rc<Self>, menu: ContextMenu) {
        let slot: Rc<RefCell<Option<ContextMenu>>> = Rc::new(RefCell::new(Some(menu)));

        let entries: Vec<(String, ContextMenuAction, bool)> = slot
            .borrow()
            .as_ref()
            .map(|menu| {
                menu.items()
                    .iter()
                    .filter_map(|item| match item {
                        ContextMenuItem::Item {
                            label,
                            action,
                            enabled,
                        } => Some((label.clone(), *action, *enabled)),
                        ContextMenuItem::Separator => None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let position = match slot.borrow().as_ref() {
            Some(menu) => menu.position(),
            None => return,
        };

        let scale = self.picture.scale_factor().max(1) as f32;
        let target = gdk::Rectangle::new(
            (position.min.x as f32 / scale).round() as i32,
            (position.min.y as f32 / scale).round() as i32,
            (position.width() as f32 / scale).round().max(1.0) as i32,
            (position.height() as f32 / scale).round().max(1.0) as i32,
        );

        let popover = gtk::Popover::builder()
            .has_arrow(false)
            .autohide(true)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);

        content.add_css_class("menu");

        let mut shown = 0usize;

        for (label, action, enabled) in entries {
            let button = gtk::Button::with_label(&label);

            button.set_sensitive(enabled);
            button.add_css_class("flat");
            button.set_halign(gtk::Align::Fill);

            {
                let slot = slot.clone();
                let popover = popover.clone();

                button.connect_clicked(move |_| {
                    if let Some(menu) = slot.borrow_mut().take() {
                        menu.select(action);
                    }

                    popover.popdown();
                });
            }

            content.append(&button);

            shown += 1;
        }

        // The interface's own entries (view source, …), appended exactly as the
        // WebKit backend appends them to the engine's menu.
        let extra = self
            .hooks
            .borrow()
            .context_menu
            .as_ref()
            .and_then(|hook| hook.get());

        let entries = extra.map(|callback| callback()).unwrap_or_default();

        if !entries.is_empty() {
            content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }

        for entry in entries {
            match entry {
                MenuItem::Separator => {
                    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                }
                MenuItem::Action { label, action } => {
                    let button = gtk::Button::with_label(&label);

                    button.add_css_class("flat");
                    button.set_halign(gtk::Align::Fill);

                    {
                        let picture = self.picture.clone();
                        let popover = popover.clone();

                        button.connect_clicked(move |_| {
                            let _ = picture.activate_action(&action, None);

                            popover.popdown();
                        });
                    }

                    content.append(&button);

                    shown += 1;
                }
            }
        }

        if shown == 0 {
            // Nothing to offer: dismiss the engine's request instead of
            // leaving it waiting for a selection that can never come.
            if let Some(menu) = slot.borrow_mut().take() {
                menu.dismiss();
            }

            return;
        }

        popover.set_child(Some(&content));
        popover.set_parent(&self.picture);
        popover.set_pointing_to(Some(&target));

        {
            let slot = slot.clone();
            let popover_for_close = popover.clone();

            popover.connect_closed(move |_| {
                // Closing without a choice is a dismissal, not a selection.
                if let Some(menu) = slot.borrow_mut().take() {
                    menu.dismiss();
                }

                popover_for_close.unparent();
            });
        }

        popover.popup();
    }

    // Script dialogs -------------------------------------------------------

    fn show_simple_dialog(self: &Rc<Self>, dialog: SimpleDialog) {
        // `prompt()` needs a text-entry dialog the shared interface does not
        // have; dismissing it keeps the page's script moving.
        if matches!(&dialog, SimpleDialog::Prompt(_)) {
            dialog.dismiss();

            return;
        }

        let is_confirm = matches!(&dialog, SimpleDialog::Confirm(_));
        let message = dialog.message().to_string();

        let Some(parent) = self.picture.root().and_downcast::<gtk::Window>() else {
            dialog.dismiss();

            return;
        };

        let buttons: Vec<String> = if is_confirm {
            vec![
                rust_i18n::t!("dialog.cancel").to_string(),
                rust_i18n::t!("dialog.ok").to_string(),
            ]
        } else {
            vec![rust_i18n::t!("dialog.ok").to_string()]
        };
        let ok_index = (buttons.len() - 1) as i32;

        let alert = gtk::AlertDialog::builder()
            .modal(true)
            .message(&message)
            .buttons(buttons)
            .default_button(ok_index)
            .cancel_button(if is_confirm { 0 } else { ok_index })
            .build();

        alert.choose(Some(&parent), None::<&gio::Cancellable>, move |result| {
            if is_confirm && !matches!(result, Ok(index) if index == ok_index) {
                dialog.dismiss();
            } else {
                dialog.confirm();
            }
        });
    }
}

/// The delegate Servo reports through.
///
/// It holds the page weakly: the page owns the `WebView`, and the `WebView`
/// owns its delegate, so a strong link here would keep every closed tab's
/// engine alive for the life of the process.
struct Delegate {
    view: RefCell<Weak<ViewInner>>,
}

impl Delegate {
    fn upgrade(&self) -> Option<Rc<ViewInner>> {
        self.view.borrow().upgrade()
    }
}

impl WebViewDelegate for Delegate {
    fn notify_new_frame_ready(&self, _webview: WebView) {
        if let Some(inner) = self.upgrade() {
            inner.dirty.set(true);
        }
    }

    fn notify_url_changed(&self, _webview: WebView, url: Url) {
        if let Some(inner) = self.upgrade() {
            let uri = url.to_string();

            inner.emit_uri(Some(&uri));
        }
    }

    fn notify_page_title_changed(&self, _webview: WebView, title: Option<String>) {
        if let Some(inner) = self.upgrade() {
            inner.emit_title(title.as_deref());
        }
    }

    fn notify_load_status_changed(&self, _webview: WebView, status: LoadStatus) {
        let Some(inner) = self.upgrade() else {
            return;
        };

        // Servo reports a load starting and completing, not the redirect and
        // commit stages WebKit has: the seam's four states are collapsed to
        // `Started` and `Finished` here.
        if status == LoadStatus::Complete {
            inner.emit_load(LoadState::Finished);
        } else {
            inner.emit_load(LoadState::Started);
        }

        inner.emit_loading(status != LoadStatus::Complete);
    }

    fn request_navigation(&self, _webview: WebView, request: NavigationRequest) {
        let Some(inner) = self.upgrade() else {
            return;
        };

        let uri = request.url.to_string();

        if BrowserEngine::is_external_scheme(&uri) {
            // The page must not be able to start an arbitrary protocol handler
            // by navigating to it, so the navigation is refused and the
            // desktop is asked about the link instead.
            BrowserEngine::open_external(&View { inner }, &uri);

            request.deny();
        } else {
            request.allow();
        }
    }

    fn request_permission(&self, _webview: WebView, request: PermissionRequest) {
        // The permission prompt in this tree is built on WebKit's request
        // kinds; Servo reports its own `PermissionFeature` set and has no
        // shared shape to reuse. Denying is the safe default and settles the
        // page's promise instead of leaving it pending.
        eprintln!(
            "Denied web permission ({:?}): this backend has no prompt yet",
            request.feature()
        );

        request.deny();
    }

    fn request_authentication(&self, _webview: WebView, request: AuthenticationRequest) {
        let Some(inner) = self.upgrade() else {
            return;
        };

        let host = request.url().host_str().unwrap_or_default().to_string();
        let pending: Rc<RefCell<Option<AuthenticationRequest>>> =
            Rc::new(RefCell::new(Some(request)));

        let prompt = AuthPrompt::new(
            host,
            // Servo does not carry the realm or a retry flag, and proposing a
            // username would guess.
            String::new(),
            false,
            None,
            Box::new(move |credentials| {
                let Some(request) = pending.borrow_mut().take() else {
                    return;
                };

                if let Some(credentials) = credentials {
                    request.authenticate(credentials.username, credentials.password);
                }

                // No credentials: dropping the request cancels it.
            }),
        );

        inner.emit_authenticate(prompt);
    }

    fn request_create_new(&self, _parent: WebView, request: CreateNewWebViewRequest) {
        // `window.open()` needs a second `WebView` wired into a second tab, and
        // the shared interface creates tabs from a URI — which this request
        // does not carry. Dropping it closes the popup instead of leaking a
        // page nobody can see.
        eprintln!("Closed a window.open() request: popups are not supported by this backend yet");

        drop(request);
    }

    fn show_embedder_control(&self, _webview: WebView, control: EmbedderControl) {
        let Some(inner) = self.upgrade() else {
            return;
        };

        match control {
            EmbedderControl::ContextMenu(menu) => inner.show_context_menu(menu),
            EmbedderControl::SimpleDialog(dialog) => inner.show_simple_dialog(dialog),
            // Select, colour and file pickers plus the IME need native dialogs
            // the shared interface does not have; dropping the request settles
            // it without leaving the page waiting.
            _ => {}
        }
    }

    fn show_notification(&self, _webview: WebView, notification: Notification) {
        if let Some(inner) = self.upgrade() {
            let callback = inner
                .hooks
                .borrow()
                .notification
                .as_ref()
                .and_then(|hook| hook.get());

            if let Some(callback) = callback {
                callback(notification.title, notification.body);
            }
        }
    }

    fn notify_crashed(&self, _webview: WebView, reason: String, _backtrace: Option<String>) {
        // A crashed page has no document left to draw an error into; reloading
        // respawns the engine, which is what the user tries first anyway.
        eprintln!("Servo page crashed: {reason}");
    }
}

/// Maps GTK's button numbers to Servo's.
fn mouse_button(button: u32) -> MouseButton {
    match button {
        // GDK numbers: 1 primary, 2 middle, 3 secondary, 8 back, 9 forward.
        1 => MouseButton::Primary,
        2 => MouseButton::Auxiliary,
        3 => MouseButton::Secondary,
        8 => MouseButton::Back,
        9 => MouseButton::Forward,
        other => MouseButton::Other(u16::try_from(other).unwrap_or(u16::MAX)),
    }
}

/// Translates a GTK key event into Servo's.
///
/// Printable characters travel as `Key::Character`, which is what the DOM sees
/// as `event.key`; everything else is a named key. The physical `Code` is left
/// unidentified: GDK keyvals do not carry the W3C code, and guessing it from
/// the keyval would be wrong on non-US layouts.
fn keyboard_event(
    state: KeyState,
    keyval: gdk::Key,
    modifiers: gdk::ModifierType,
    repeat: bool,
) -> Option<KeyboardEvent> {
    let logical = match keyval.to_unicode() {
        Some(character) if !character.is_control() => Key::Character(character.to_string()),
        _ => Key::Named(named_key(keyval)?),
    };

    Some(KeyboardEvent::new_without_event(
        state,
        logical,
        Code::Unidentified,
        Location::Standard,
        servo_modifiers(modifiers),
        repeat,
        false,
    ))
}

fn servo_modifiers(state: gdk::ModifierType) -> Modifiers {
    let mut modifiers = Modifiers::empty();

    modifiers.set(
        Modifiers::SHIFT,
        state.contains(gdk::ModifierType::SHIFT_MASK),
    );
    modifiers.set(
        Modifiers::CONTROL,
        state.contains(gdk::ModifierType::CONTROL_MASK),
    );
    modifiers.set(Modifiers::ALT, state.contains(gdk::ModifierType::ALT_MASK));
    modifiers.set(
        Modifiers::META,
        state.contains(gdk::ModifierType::SUPER_MASK),
    );

    modifiers
}

/// The named keys a browser page actually uses (form controls, scrolling,
/// editing). Compared with `==` rather than matched: GDK's keys are constants,
/// not an enum, so a comparison chain is the shape that cannot depend on how
/// they are declared.
fn named_key(keyval: gdk::Key) -> Option<NamedKey> {
    if keyval == gdk::Key::Return || keyval == gdk::Key::KP_Enter {
        return Some(NamedKey::Enter);
    }

    if keyval == gdk::Key::Tab {
        return Some(NamedKey::Tab);
    }

    if keyval == gdk::Key::BackSpace {
        return Some(NamedKey::Backspace);
    }

    if keyval == gdk::Key::Escape {
        return Some(NamedKey::Escape);
    }

    if keyval == gdk::Key::Delete {
        return Some(NamedKey::Delete);
    }

    if keyval == gdk::Key::Insert {
        return Some(NamedKey::Insert);
    }

    if keyval == gdk::Key::Home {
        return Some(NamedKey::Home);
    }

    if keyval == gdk::Key::End {
        return Some(NamedKey::End);
    }

    if keyval == gdk::Key::Page_Up {
        return Some(NamedKey::PageUp);
    }

    if keyval == gdk::Key::Page_Down {
        return Some(NamedKey::PageDown);
    }

    if keyval == gdk::Key::Left {
        return Some(NamedKey::ArrowLeft);
    }

    if keyval == gdk::Key::Right {
        return Some(NamedKey::ArrowRight);
    }

    if keyval == gdk::Key::Up {
        return Some(NamedKey::ArrowUp);
    }

    if keyval == gdk::Key::Down {
        return Some(NamedKey::ArrowDown);
    }

    if keyval == gdk::Key::Shift_L || keyval == gdk::Key::Shift_R {
        return Some(NamedKey::Shift);
    }

    if keyval == gdk::Key::Control_L || keyval == gdk::Key::Control_R {
        return Some(NamedKey::Control);
    }

    if keyval == gdk::Key::Alt_L || keyval == gdk::Key::Alt_R {
        return Some(NamedKey::Alt);
    }

    if keyval == gdk::Key::F5 {
        return Some(NamedKey::F5);
    }

    if keyval == gdk::Key::F11 {
        return Some(NamedKey::F11);
    }

    if keyval == gdk::Key::F12 {
        return Some(NamedKey::F12);
    }

    None
}

/// Registers the interface's callbacks for a page.
///
/// `open_in_new_tab` is accepted and unused: it exists for parity with the
/// WebKit backend, where it opens a popup in a tab, and this backend cannot
/// (Servo's `request_create_new` carries no URL to open). `show_notification`
/// and `on_authenticate` are honoured by the delegate.
pub(crate) fn wire(
    view: &View,
    _open_in_new_tab: Rc<dyn Fn(String)>,
    show_notification: Rc<dyn Fn(String, String)>,
    on_authenticate: AuthCallback,
) {
    let mut hooks = view.inner.hooks.borrow_mut();

    hooks.notification = Some(Hook::new(show_notification));
    hooks.authenticate = Some(Hook::new(on_authenticate));
}

/// Opens the load-failure page for a watchdog timeout.
pub(crate) fn show_timeout_page(view: &View, uri: &str) {
    let title = rust_i18n::t!("error.load_title").to_string();
    let detail = rust_i18n::t!("error.timeout_detail").to_string();

    view.inner.show_error_page(uri, &title, &detail);
}

/// Returns true (and clears the mark) when `uri` is the URI of an error page
/// currently shown, so history recording can skip it.
pub(crate) fn take_error_page(view: &View, uri: &str) -> bool {
    view.inner.take_error_page(uri)
}

pub(crate) fn mark_source_view(view: &View) {
    view.inner.mark_source_view();
}

pub(crate) fn take_source_view(view: &View) -> bool {
    view.inner.take_source_view()
}

/// Applies the settings a page is created with.
///
/// The seam requires the call; this backend has nothing to apply it to —
/// developer extras and the GPU choice have no Servo 0.7 equivalent — so the
/// settings are dropped here, once, instead of being stored as dead state.
pub(crate) fn configure(_view: &View, _config: &ViewConfig) {}
