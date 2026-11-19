//! axysBrowser → Servo on Windows: phase-1 proof of concept.
//!
//! This is not the browser. It is the smallest program that answers, on a real
//! Windows desktop, the questions the port depends on:
//!
//! * Servo initialises in-process (no external browser, no Chromium/CEF/WebView2).
//! * A rendering context serves frames: `WindowRenderingContext` on a native
//!   HWND, or `SoftwareRenderingContext` with no window at all.
//! * Navigation, reload and zoom work.
//! * JavaScript runs and its results come back into Rust.
//! * Mouse, wheel, keyboard and resize events reach the page.
//! * Several `WebView`s can exist at once against one `Servo` instance.
//! * Tearing a window down really frees its `WebView`: the delegate holds a
//!   `Weak` reference, so no `Rc` cycle keeps a closed page's engine alive.
//!
//! Usage:
//!
//! ```text
//! cargo run                        # opens the Servo demo page
//! cargo run -- https://example.com
//! cargo run -- --webviews 3        # three windows = three concurrent WebViews
//! cargo run -- --self-test         # windowed assertion run, exit 0 = pass
//! cargo run -- --self-test --headless   # no window, software rendering (CI)
//! ```
//!
//! `--headless` is what CI runs: it needs no GPU and no desktop session, so the
//! gate tests the engine, not the runner's display.
//!
//! In-window shortcuts (browser-level, checked before the page): Ctrl+R reload,
//! Ctrl+L load a second URL (stands in for the address bar), Ctrl+J prove
//! `evaluate_javascript`, Ctrl+F inject a find probe, Ctrl+±/0 zoom.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::error::Error;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use servo::{
    Code, DeviceIntRect, DevicePoint, EventLoopWaker, InputEvent, JSValue, Key, KeyState,
    KeyboardEvent, LoadStatus, Location, Modifiers, MouseButton, MouseButtonAction,
    MouseButtonEvent, MouseMoveEvent, RenderingContext, Servo, ServoBuilder,
    SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate, WebViewPoint, WheelDelta,
    WheelEvent, WheelMode, WindowRenderingContext,
};
use url::Url;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent as WinitKeyEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WinitKey, KeyCode, NamedKey, PhysicalKey};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Window, WindowId};

const DEMO_URL: &str = "https://demo.servo.org/";
const SELFTEST_URL: &str = "data:text/html,<title>axys-servo-poc</title><h1>ok</h1>\
<script>document.title='js-ok:'+(1+1);</script>";
const SELFTEST_EXPECTED: &str = "js-ok:2";
/// A hung self-test must fail the job instead of burning the CI timeout.
const SELFTEST_DEADLINE: Duration = Duration::from_secs(120);

fn main() -> Result<(), Box<dyn Error>> {
    // Servo's network stack needs a process-wide rustls provider, fixed up front
    // so the choice cannot drift silently between builds.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| "a crypto provider was already installed")?;

    let options = Options::from_args(std::env::args().skip(1))?;

    let event_loop = EventLoop::<WakeEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let shared = Rc::new(Shared::default());

    let servo = ServoBuilder::default()
        .event_loop_waker(Box::new(Waker(proxy.clone())))
        .build();
    servo.setup_logging();

    let mut app = App {
        servo,
        proxy,
        options,
        shared: shared.clone(),
        states: HashMap::new(),
        started: None,
    };

    event_loop.run_app(&mut app)?;

    // Nothing releases Servo from inside the running loop; dropping the last
    // handle after `exiting` is the documented teardown path in servo 0.7.
    drop(app);

    let code = shared.exit_code.get();
    std::process::exit(code);
}

#[derive(Debug, Clone)]
struct Options {
    url: Url,
    windows: usize,
    self_test: bool,
    headless: bool,
}

impl Options {
    fn from_args(args: impl Iterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut url = None;
        let mut windows = 1usize;
        let mut self_test = false;
        let mut headless = false;
        let mut args = args;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--self-test" => self_test = true,
                "--headless" => {
                    headless = true;
                    self_test = true;
                }
                "--webviews" => {
                    let value = args.next().ok_or("--webviews needs a number")?;
                    windows = value.parse::<usize>()?.clamp(1, 8);
                }
                other => {
                    url = Some(
                        Url::parse(other).or_else(|_| Url::parse(&format!("https://{other}")))?,
                    );
                }
            }
        }

        if self_test || headless {
            windows = 1;
        }

        let url = match url {
            Some(url) => url,
            None if self_test => Url::parse(SELFTEST_URL).expect("static URL"),
            None => Url::parse(DEMO_URL).expect("static URL"),
        };

        Ok(Self {
            url,
            windows,
            self_test,
            headless,
        })
    }
}

/// State shared by every window and its delegate.
#[derive(Default)]
struct Shared {
    exit_code: Cell<i32>,
    /// Set once a self-test reached a verdict.
    verdict: Cell<Option<bool>>,
}

/// Where a self-test is in its sequence: the page has to load, then JavaScript
/// has to prove itself, then the engine has to have produced pixels.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Loading,
    NeedPixels,
    Done,
}

// ---------------------------------------------------------------------------
// Event-loop waker
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WakeEvent;

/// Servo asks the embedder to re-enter its event loop, possibly from another
/// thread; winit's proxy turns that into a `user_event` on the UI thread.
#[derive(Clone)]
struct Waker(EventLoopProxy<WakeEvent>);

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        // A closed loop is expected during shutdown, so this must not panic.
        let _ = self.0.send_event(WakeEvent);
    }
}

// ---------------------------------------------------------------------------
// Per-window (or headless) state
// ---------------------------------------------------------------------------

struct AppState {
    /// `None` in headless mode: the engine renders with no window at all.
    window: Option<Window>,
    servo: Servo,
    rendering_context: Rc<dyn RenderingContext>,
    delegate: Rc<Delegate>,
    webview: RefCell<Option<WebView>>,
    /// Last cursor position, because winit's `MouseInput` carries no coordinates.
    pointer: Cell<(f64, f64)>,
    modifiers: Cell<Modifiers>,
    zoom: Cell<f32>,
    report: WindowReport,
    stage: Cell<Stage>,
    /// Headless mode cannot rely on a redraw request, so it paints from the
    /// event loop when Servo says a frame is ready.
    needs_paint: Cell<bool>,
}

#[derive(Default)]
struct WindowReport {
    url: RefCell<Option<String>>,
    title: RefCell<Option<String>>,
    loaded: Cell<bool>,
    js_result: RefCell<Option<String>>,
    pixels: Cell<bool>,
    painted_frames: Cell<u32>,
}

impl WindowReport {
    fn print(&self) {
        println!(
            "[report] url={:?} title={:?} loaded={} js={:?} pixels={} frames={}",
            self.url.borrow(),
            self.title.borrow(),
            self.loaded.get(),
            self.js_result.borrow(),
            self.pixels.get(),
            self.painted_frames.get()
        );
    }
}

/// The delegate must not own `AppState`: `AppState` owns the `WebView`, and the
/// `WebView` owns its delegate. A strong reference here would keep every closed
/// tab's engine alive for the life of the process — the exact memory regression
/// this port has to avoid — so the link is a `Weak`.
struct Delegate {
    state: RefCell<Weak<AppState>>,
    proxy: EventLoopProxy<WakeEvent>,
    self_test: bool,
    shared: Rc<Shared>,
}

impl WebViewDelegate for Delegate {
    fn notify_new_frame_ready(&self, _webview: WebView) {
        let Some(state) = self.state.borrow().upgrade() else {
            return;
        };
        let frames = state.report.painted_frames.get();
        state.report.painted_frames.set(frames + 1);

        match state.window.as_ref() {
            Some(window) => window.request_redraw(),
            None => {
                state.needs_paint.set(true);
                let _ = self.proxy.send_event(WakeEvent);
            }
        }
    }

    fn notify_url_changed(&self, webview: WebView, url: Url) {
        if let Some(state) = self.state.borrow().upgrade() {
            println!("[url] {url}");
            *state.report.url.borrow_mut() = Some(url.to_string());
            *state.report.title.borrow_mut() = webview.page_title();
        }
    }

    fn notify_page_title_changed(&self, _webview: WebView, title: Option<String>) {
        if let Some(state) = self.state.borrow().upgrade() {
            if let (Some(window), Some(title)) = (state.window.as_ref(), title.as_ref()) {
                window.set_title(&format!("axys servo poc — {title}"));
            }
            *state.report.title.borrow_mut() = title;
        }
    }

    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        println!("[load] {status:?}");
        let Some(state) = self.state.borrow().upgrade() else {
            return;
        };
        if status != LoadStatus::Complete {
            return;
        }
        state.report.loaded.set(true);

        // Prove JavaScript runs and its value crosses back into Rust.
        let weak = Rc::downgrade(&state);
        let shared = self.shared.clone();
        let self_test = self.self_test;
        let proxy = self.proxy.clone();
        webview.evaluate_javascript("document.title", move |result| {
            let text = match result {
                Ok(JSValue::String(value)) => value,
                Ok(other) => format!("{other:?}"),
                Err(error) => {
                    eprintln!("[js] evaluation failed: {error}");
                    String::new()
                }
            };
            println!("[js] document.title = {text:?}");

            let Some(state) = weak.upgrade() else {
                return;
            };
            *state.report.js_result.borrow_mut() = Some(text.clone());

            if !self_test {
                return;
            }

            if text == SELFTEST_EXPECTED {
                // JavaScript is right; now require the engine to have drawn.
                state.stage.set(Stage::NeedPixels);
                let _ = proxy.send_event(WakeEvent);
            } else {
                state.stage.set(Stage::Done);
                shared.exit_code.set(1);
                shared.verdict.set(Some(false));
                let _ = proxy.send_event(WakeEvent);
            }
        });
    }

    fn notify_crashed(&self, _webview: WebView, reason: String, backtrace: Option<String>) {
        eprintln!("[crash] {reason}");
        if let Some(backtrace) = backtrace {
            eprintln!("{backtrace}");
        }
        self.shared.exit_code.set(1);
        self.shared.verdict.set(Some(false));
        let _ = self.proxy.send_event(WakeEvent);
    }

    fn notify_closed(&self, _webview: WebView) {
        println!("[closed] webview released");
    }
}

impl AppState {
    fn opened(
        window: Option<Window>,
        rendering_context: Rc<dyn RenderingContext>,
        servo: Servo,
        proxy: EventLoopProxy<WakeEvent>,
        options: &Options,
        shared: Rc<Shared>,
    ) -> Rc<Self> {
        let state = Rc::new(Self {
            window,
            servo,
            rendering_context,
            delegate: Rc::new(Delegate {
                state: RefCell::new(Weak::new()),
                proxy,
                self_test: options.self_test,
                shared,
            }),
            webview: RefCell::new(None),
            pointer: Cell::new((0.0, 0.0)),
            modifiers: Cell::new(Modifiers::empty()),
            zoom: Cell::new(1.0),
            report: WindowReport::default(),
            stage: Cell::new(Stage::Loading),
            needs_paint: Cell::new(false),
        });
        *state.delegate.state.borrow_mut() = Rc::downgrade(&state);
        state
    }

    fn new_headed(
        event_loop: &ActiveEventLoop,
        servo: Servo,
        proxy: EventLoopProxy<WakeEvent>,
        options: &Options,
        shared: Rc<Shared>,
        index: usize,
    ) -> Result<Rc<Self>, Box<dyn Error>> {
        let display_handle = event_loop
            .display_handle()
            .map_err(|error| format!("no display handle: {error}"))?;

        let title = if index <= 1 {
            "axys servo poc".to_string()
        } else {
            format!("axys servo poc #{index}")
        };

        let window = event_loop.create_window(
            Window::default_attributes()
                .with_title(title)
                .with_inner_size(PhysicalSize::new(1024, 768)),
        )?;
        let window_handle = window
            .window_handle()
            .map_err(|error| format!("no window handle: {error}"))?;

        let rendering_context: Rc<dyn RenderingContext> = Rc::new(WindowRenderingContext::new(
            display_handle,
            window_handle,
            window.inner_size(),
        )?);
        rendering_context.make_current()?;

        let state = Self::opened(
            Some(window),
            rendering_context,
            servo,
            proxy,
            options,
            shared,
        );
        state.boot(&options.url);
        Ok(state)
    }

    /// No window and no GPU: Servo renders through a software context, which is
    /// what CI needs and what the GTK4 texture bridge will use.
    fn new_headless(
        servo: Servo,
        proxy: EventLoopProxy<WakeEvent>,
        options: &Options,
        shared: Rc<Shared>,
    ) -> Result<Rc<Self>, Box<dyn Error>> {
        let size = PhysicalSize::new(1024, 768);
        let rendering_context: Rc<dyn RenderingContext> =
            Rc::new(SoftwareRenderingContext::new(size)?);
        rendering_context.make_current()?;

        let state = Self::opened(None, rendering_context, servo, proxy, options, shared);
        state.boot(&options.url);
        Ok(state)
    }

    fn boot(&self, url: &Url) {
        let webview = WebViewBuilder::new(&self.servo, self.rendering_context.clone())
            .url(url.clone())
            .delegate(self.delegate.clone())
            .build();
        webview.set_focused(true);
        *self.webview.borrow_mut() = Some(webview);
        self.servo.spin_event_loop();
    }

    fn webview(&self) -> Option<WebView> {
        self.webview.borrow().clone()
    }

    fn paint(&self) {
        if let Some(webview) = self.webview() {
            webview.paint();
            self.rendering_context.present();
        }
    }

    /// Reads the rendered frame back and reports whether anything was drawn.
    /// This is what turns "the page loaded" into "the engine rendered".
    fn saw_pixels(&self) -> bool {
        self.paint();
        let size = self.rendering_context.size();
        let rect: DeviceIntRect = euclid::Box2D::new(
            euclid::Point2D::new(0i32, 0i32),
            euclid::Point2D::new(size.width as i32, size.height as i32),
        );
        let Some(image) = self.rendering_context.read_to_image(rect) else {
            return false;
        };
        // Any meaningful colour beats a blank buffer; the test page is white.
        image
            .as_raw()
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 8 || pixel[1] > 8 || pixel[2] > 8)
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

    /// Releases the page now instead of at process exit. Servo calls
    /// `notify_closed` only when the last handle is gone, so this is what the
    /// memory smoke test observes.
    fn close_webview(&self) {
        let taken = self.webview.borrow_mut().take();
        if let Some(webview) = taken {
            webview.set_focused(false);
            drop(webview);
        }
    }
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

struct App {
    servo: Servo,
    proxy: EventLoopProxy<WakeEvent>,
    options: Options,
    shared: Rc<Shared>,
    states: HashMap<Option<WindowId>, Rc<AppState>>,
    started: Option<Instant>,
}

impl App {
    fn open_window(&mut self, event_loop: &ActiveEventLoop) {
        let index = self.states.len() + 1;
        let result = AppState::new_headed(
            event_loop,
            self.servo.clone(),
            self.proxy.clone(),
            &self.options,
            self.shared.clone(),
            index,
        );

        match result {
            Ok(state) => {
                println!("[window] opened #{index}");
                let id = state.window.as_ref().map(Window::id);
                self.states.insert(id, state);
            }
            Err(error) => {
                eprintln!("[window] failed to open: {error}");
                self.shared.exit_code.set(1);
                self.shared.verdict.set(Some(false));
                event_loop.exit();
            }
        }
    }

    fn open_headless(&mut self, event_loop: &ActiveEventLoop) {
        match AppState::new_headless(
            self.servo.clone(),
            self.proxy.clone(),
            &self.options,
            self.shared.clone(),
        ) {
            Ok(state) => {
                println!("[headless] software context 1024x768");
                self.states.insert(None, state);
            }
            Err(error) => {
                eprintln!("[headless] failed to start: {error}");
                self.shared.exit_code.set(1);
                self.shared.verdict.set(Some(false));
                event_loop.exit();
            }
        }
    }

    fn initialise(&mut self, event_loop: &ActiveEventLoop) {
        if !self.states.is_empty() {
            return;
        }
        self.started = Some(Instant::now());

        if self.options.headless {
            self.open_headless(event_loop);
        } else {
            for _ in 0..self.options.windows.max(1) {
                self.open_window(event_loop);
            }
        }
    }

    /// Drives the self-test forward and enforces its deadline.
    fn poll_self_test(&mut self, event_loop: &ActiveEventLoop) {
        if !self.options.self_test {
            return;
        }

        for state in self.states.values() {
            match state.stage.get() {
                Stage::NeedPixels => {
                    let drew = state.saw_pixels();
                    state.report.pixels.set(drew);
                    state.stage.set(Stage::Done);
                    state.report.print();
                    println!(
                        "[self-test] rendering {}",
                        if drew { "ok" } else { "empty" }
                    );
                    self.shared.exit_code.set(if drew { 0 } else { 1 });
                    self.shared.verdict.set(Some(drew));
                }
                Stage::Loading | Stage::Done => {}
            }
        }

        if let Some(passed) = self.shared.verdict.get() {
            println!("[self-test] {}", if passed { "PASS" } else { "FAIL" });
            event_loop.exit();
            return;
        }

        let expired = self
            .started
            .map(|started| started.elapsed() > SELFTEST_DEADLINE)
            .unwrap_or(false);
        if expired {
            for state in self.states.values() {
                state.report.print();
            }
            println!("[self-test] FAIL (timed out after {SELFTEST_DEADLINE:?})");
            self.shared.exit_code.set(1);
            self.shared.verdict.set(Some(false));
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<WakeEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.initialise(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: WakeEvent) {
        self.servo.spin_event_loop();
        self.poll_self_test(event_loop);
    }

    /// A headless run has no window events at all, so the loop is also pumped
    /// here; this is what keeps a windowless CI run alive.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.initialise(event_loop);

        if self.options.headless {
            self.servo.spin_event_loop();
            for state in self.states.values() {
                if state.needs_paint.take() {
                    state.paint();
                }
            }
        }

        self.poll_self_test(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.states.get(&Some(window_id)).cloned() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                state.report.print();
                // Resource release check: the engine's page and its memory must
                // go with this window, not with the process.
                state.close_webview();
                self.servo.spin_event_loop();
                self.states.remove(&Some(window_id));
                if self.states.is_empty() {
                    event_loop.exit();
                }
            }

            WindowEvent::RedrawRequested => state.paint(),

            WindowEvent::Resized(size) => {
                if let Some(webview) = state.webview() {
                    webview.resize(size);
                }
                self.servo.spin_event_loop();
            }

            WindowEvent::Focused(focused) => {
                if let Some(webview) = state.webview() {
                    webview.set_focused(focused);
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.pointer.set((position.x, position.y));
                state.send(InputEvent::MouseMove(MouseMoveEvent::new(state.point())));
                self.servo.spin_event_loop();
            }

            WindowEvent::MouseInput {
                state: key_state,
                button,
                ..
            } => {
                let action = match key_state {
                    ElementState::Pressed => MouseButtonAction::Down,
                    ElementState::Released => MouseButtonAction::Up,
                };
                let button = match button {
                    winit::event::MouseButton::Left => MouseButton::Primary,
                    winit::event::MouseButton::Right => MouseButton::Secondary,
                    winit::event::MouseButton::Middle => MouseButton::Auxiliary,
                    winit::event::MouseButton::Back => MouseButton::Back,
                    winit::event::MouseButton::Forward => MouseButton::Forward,
                    winit::event::MouseButton::Other(other) => MouseButton::Other(other),
                };
                state.send(InputEvent::MouseButton(MouseButtonEvent::new(
                    action,
                    button,
                    state.point(),
                )));
                self.servo.spin_event_loop();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y, mode) = match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => {
                        ((dx * 76.0) as f64, (dy * 76.0) as f64, WheelMode::DeltaLine)
                    }
                    MouseScrollDelta::PixelDelta(delta) => {
                        (delta.x, delta.y, WheelMode::DeltaPixel)
                    }
                };
                state.send(InputEvent::Wheel(WheelEvent::new(
                    WheelDelta { x, y, z: 0.0, mode },
                    state.point(),
                )));
                self.servo.spin_event_loop();
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                let mut flags = Modifiers::empty();
                flags.set(Modifiers::SHIFT, modifiers.state().shift_key());
                flags.set(Modifiers::CONTROL, modifiers.state().control_key());
                flags.set(Modifiers::ALT, modifiers.state().alt_key());
                flags.set(Modifiers::META, modifiers.state().super_key());
                state.modifiers.set(flags);
            }

            WindowEvent::KeyboardInput { event: key, .. } => {
                // Browser shortcuts come first, as on Linux: a page must not be
                // able to swallow Ctrl+W.
                let modifiers = state.modifiers.get();
                if key.state == ElementState::Pressed
                    && modifiers.contains(Modifiers::CONTROL)
                    && shortcut(&state, &key)
                {
                    self.servo.spin_event_loop();
                    return;
                }

                if let Some(event) = keyboard_event(&key, modifiers) {
                    state.send(InputEvent::Keyboard(event));
                    self.servo.spin_event_loop();
                }
            }

            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        for state in self.states.values() {
            state.close_webview();
        }
        self.servo.spin_event_loop();
        self.states.clear();
    }
}

fn keyboard_event(key: &WinitKeyEvent, modifiers: Modifiers) -> Option<KeyboardEvent> {
    let state = match key.state {
        ElementState::Pressed => KeyState::Down,
        ElementState::Released => KeyState::Up,
    };

    let logical = match &key.logical_key {
        WinitKey::Character(text) if !text.is_empty() => Key::Character(text.to_string()),
        WinitKey::Named(named) => match named_key(*named) {
            Some(named) => Key::Named(named),
            None => return None,
        },
        _ => match key.text.as_ref() {
            Some(text) if !text.is_empty() => Key::Character(text.to_string()),
            _ => return None,
        },
    };

    // winit and `keyboard_types` share the W3C UI Events names, so the Debug
    // form of the physical key round-trips through `Code::from_str`.
    let code = match key.physical_key {
        PhysicalKey::Code(code) => format!("{code:?}")
            .parse::<Code>()
            .unwrap_or(Code::Unidentified),
        PhysicalKey::Unidentified(_) => Code::Unidentified,
    };

    Some(KeyboardEvent::new_without_event(
        state,
        logical,
        code,
        Location::Standard,
        modifiers,
        key.repeat,
        false,
    ))
}

fn named_key(named: NamedKey) -> Option<servo::NamedKey> {
    use servo::NamedKey as S;
    Some(match named {
        NamedKey::Enter => S::Enter,
        NamedKey::Tab => S::Tab,
        NamedKey::Backspace => S::Backspace,
        NamedKey::Escape => S::Escape,
        NamedKey::Delete => S::Delete,
        NamedKey::Insert => S::Insert,
        NamedKey::Home => S::Home,
        NamedKey::End => S::End,
        NamedKey::PageUp => S::PageUp,
        NamedKey::PageDown => S::PageDown,
        NamedKey::ArrowLeft => S::ArrowLeft,
        NamedKey::ArrowRight => S::ArrowRight,
        NamedKey::ArrowUp => S::ArrowUp,
        NamedKey::ArrowDown => S::ArrowDown,
        NamedKey::Shift => S::Shift,
        NamedKey::Control => S::Control,
        NamedKey::Alt => S::Alt,
        NamedKey::F5 => S::F5,
        NamedKey::F11 => S::F11,
        NamedKey::F12 => S::F12,
        _ => return None,
    })
}

/// Returns true when the browser consumed the key.
fn shortcut(state: &Rc<AppState>, key: &WinitKeyEvent) -> bool {
    let Some(webview) = state.webview() else {
        return false;
    };
    let PhysicalKey::Code(code) = key.physical_key else {
        return false;
    };

    match code {
        KeyCode::KeyR => {
            println!("[shortcut] reload");
            webview.reload();
            true
        }
        KeyCode::KeyJ => {
            webview.evaluate_javascript("1+1", |result| println!("[js] 1+1 = {result:?}"));
            true
        }
        KeyCode::KeyL => {
            // Stands in for the address bar: proves chrome-driven navigation.
            let url =
                Url::parse(&format!("{DEMO_URL}experiments/twgl-tunnel/")).expect("static URL");
            println!("[shortcut] load {url}");
            webview.load(url);
            true
        }
        KeyCode::Equal | KeyCode::NumpadAdd => {
            let zoom = (state.zoom.get() * 1.1).min(5.0);
            state.zoom.set(zoom);
            webview.set_page_zoom(zoom);
            true
        }
        KeyCode::Minus | KeyCode::NumpadSubtract => {
            let zoom = (state.zoom.get() / 1.1).max(0.3);
            state.zoom.set(zoom);
            webview.set_page_zoom(zoom);
            true
        }
        KeyCode::Digit0 => {
            state.zoom.set(1.0);
            webview.set_page_zoom(1.0);
            true
        }
        KeyCode::KeyF => {
            webview.evaluate_javascript(
                "document.body.insertAdjacentHTML('beforeend', '<p id=axys-find>find-probe</p>');\
                 document.getElementById('axys-find').textContent",
                |result| println!("[js] find probe = {result:?}"),
            );
            true
        }
        _ => false,
    }
}
