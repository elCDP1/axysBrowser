//! The Windows backend: Servo.
//!
//! Servo is embedded as a Rust library; there is no second process and no
//! system web view. This module owns the pieces that are process-wide — the one
//! `Servo` instance, the event-loop pump, and the engine-neutral policies the
//! interface calls through `BrowserEngine`:
//!
//! * [`servo`] builds the engine on first use, with a waker that turns Servo's
//!   "spin the loop" request into a GTK main-loop wakeup.
//! * [`pump`] runs every 16 ms: it spins Servo's loop and paints the pages that
//!   reported a new frame. See the note on cost in `docs/porting/windows-servo.md`.
//! * `BrowserEngine` keeps the same surface the WebKitGTK backend exposes, so
//!   `ui/` compiles against it unchanged.
//!
//! What this backend does not do is listed in `docs/porting/windows-servo.md`,
//! next to the reason: no downloads, print, MHTML, find, DevTools, per-page
//! mute, isolated private windows or tracking prevention, because Servo 0.7's
//! embedder API has no entry point for them.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use servo::{EventLoopWaker, Servo, ServoBuilder};

use crate::browser::engine::shared;
use crate::browser::engine::types::{AuthPrompt, ViewConfig};
use crate::browser::permissions::PermissionManager;

mod download;
mod session;
mod view;

pub use download::Download;
pub use session::Session;
pub use view::{View, WeakView};

use view::ViewInner;

pub struct BrowserEngine;

/// How the interface answers an HTTP/proxy authentication request.
pub type AuthenticateHandler = Rc<dyn Fn(&View, AuthPrompt)>;

/// How often the pump spins Servo and paints dirty pages.
///
/// 60 Hz: a page that animates is redrawn at screen rate, and the cost is one
/// loop spin plus a paint of the dirty pages. A wake-driven pump would only run
/// when Servo asks for a frame, which is the documented improvement; until then
/// this timer is what keeps a page alive while nothing else happens.
const PUMP_INTERVAL: Duration = Duration::from_millis(16);

/// Directory the WebKit backend fills with its HTTP disk cache.
///
/// Servo 0.7's HTTP cache is in memory only (its `NetworkManager` says so), so
/// nothing is written here. The path still exists because the interface's
/// "clear cache" wipes it, and a wipe of an empty directory is correct.
pub fn web_cache_dir() -> PathBuf {
    crate::paths::cache_dir().join("web-cache")
}

/// Servo asks the embedder to re-enter its event loop, possibly from another
/// thread. `MainContext::wakeup` is the one thread-safe way to do that without
/// touching the GTK tree from a worker.
struct Waker {
    context: glib::MainContext,
}

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(Self {
            context: self.context.clone(),
        })
    }

    fn wake(&self) {
        self.context.wakeup();
    }
}

/// The process-wide engine: one `Servo`, and the pages it is drawing.
///
/// Visible where `engine()` is: the session and the pages take an `Rc` of it.
pub(in crate::browser::engine) struct Engine {
    servo: Servo,
    /// Pages the pump paints. Weak, so a closed tab is freed as soon as the
    /// interface drops its handle instead of at process exit.
    views: RefCell<Vec<Weak<ViewInner>>>,
}

thread_local! {
    static ENGINE: RefCell<Option<Rc<Engine>>> = const { RefCell::new(None) };
}

/// The engine, created on first use.
pub(crate) fn engine() -> Rc<Engine> {
    ENGINE.with(|slot| {
        let mut slot = slot.borrow_mut();

        slot.get_or_insert_with(create_engine).clone()
    })
}

/// The Servo handle, for the session and for the pages it creates.
pub(crate) fn servo() -> Servo {
    engine().servo.clone()
}

fn create_engine() -> Rc<Engine> {
    // Servo's network stack needs a process-wide rustls provider. Installing it
    // twice is not an error worth stopping the browser for: the PoC and any
    // future harness in the same process would race us to it.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let servo = ServoBuilder::default()
        .event_loop_waker(Box::new(Waker {
            context: glib::MainContext::default(),
        }))
        .build();

    // Without this, Servo's own logs (network failures, panics in the engine)
    // are dropped, which makes a broken page impossible to diagnose.
    servo.setup_logging();

    let engine = Rc::new(Engine {
        servo,
        views: RefCell::new(Vec::new()),
    });

    start_pump();

    engine
}

/// Registers a page so the pump paints it.
pub(crate) fn register_view(view: &Rc<ViewInner>) {
    engine().views.borrow_mut().push(Rc::downgrade(view));
}

fn start_pump() {
    let _ = glib::timeout_add_local(PUMP_INTERVAL, || {
        pump();

        glib::ControlFlow::Continue
    });
}

/// One turn of the engine: let Servo handle its events, then draw the pages
/// that reported a new frame.
fn pump() {
    let Some(engine) = ENGINE.with(|slot| slot.borrow().clone()) else {
        return;
    };

    engine.servo.spin_event_loop();

    let views: Vec<Rc<ViewInner>> = {
        let mut views = engine.views.borrow_mut();

        views.retain(|view| view.strong_count() > 0);

        views.iter().filter_map(Weak::upgrade).collect()
    };

    for view in views {
        view.paint_if_dirty();
    }
}

impl BrowserEngine {
    /// The profile's session. Creating it builds the engine on first use, and
    /// every later session reuses that one.
    pub fn create_network_session(_itp_enabled: bool) -> Session {
        let _ = engine();

        Session
    }

    /// A private window's session.
    ///
    /// Servo has one profile, so this is the profile's session and nothing is
    /// isolated; it is a separate constructor so the gap is stated once, where
    /// a private window creates it.
    pub fn create_ephemeral_session() -> Session {
        let _ = engine();

        eprintln!(
            "Private window shares Servo's single profile: cookies, cache and site data are not isolated"
        );

        Session
    }

    /// Applies the settings a page is created with.
    pub fn configure(view: &View, config: &ViewConfig) {
        view::configure(view, config);
    }

    /// Connects every engine notification a complex page needs handled.
    ///
    /// `open_in_new_tab` is kept for parity with the WebKit backend, where
    /// `window.open()` opens a tab; here the engine cannot hand over a URL, so
    /// the delegate closes such requests (documented gap).
    /// `on_authenticate` gets HTTP/proxy authentication requests.
    pub fn wire_view(
        view: &View,
        open_in_new_tab: Rc<dyn Fn(String)>,
        show_notification: Rc<dyn Fn(String, String)>,
        on_authenticate: AuthenticateHandler,
        _permissions: PermissionManager,
        _clipboard_reads: Rc<dyn Fn() -> bool>,
    ) {
        view::wire(view, open_in_new_tab, show_notification, on_authenticate);
    }

    /// Schemes the engine never loads itself: handed to the desktop instead.
    pub fn is_external_scheme(uri: &str) -> bool {
        shared::is_external_scheme(uri)
    }

    /// Hands a URI the user typed in the address bar to the desktop. Typing is
    /// the gesture, so `mailto:`/`tel:`/`sms:` open straight away and every
    /// other scheme still asks first, exactly like in-page links.
    pub fn open_external(view: &View, uri: &str) {
        if shared::external_needs_confirmation(uri) {
            confirm_external_open(view, uri);
        } else {
            launch_external(uri);
        }
    }

    pub fn load(view: &View, uri: &str) {
        view.inner().load_uri(uri);
    }

    pub fn reload(view: &View) {
        if let Some(webview) = view.inner().webview() {
            webview.reload();
        }
    }

    /// Shows the load-failure page for a locally detected timeout (used by the
    /// navigation watchdog; Servo has no load timeout of its own).
    pub fn show_timeout_page(view: &View, uri: &str) {
        view::show_timeout_page(view, uri);
    }

    /// Returns true (and clears the mark) when `uri` is the URI of an error
    /// page currently shown in `view`.
    pub fn take_error_page(view: &View, uri: &str) -> bool {
        view::take_error_page(view, uri)
    }

    /// Marks a view that is about to show a generated "view source" document,
    /// so history recording skips exactly that load.
    pub fn mark_source_view(view: &View) {
        view::mark_source_view(view);
    }

    pub fn take_source_view(view: &View) -> bool {
        view::take_source_view(view)
    }
}

/// Asks before handing a link to another application: a web page must not be
/// able to start `ssh:`, `smb:`, `vnc:` … handlers with one click the user did
/// not know would do that.
fn confirm_external_open(view: &View, uri: &str) {
    let Some(parent) = view.widget().root().and_downcast::<gtk::Window>() else {
        eprintln!(
            "No window to ask about opening {}; not opening it",
            crate::browser::util::log_safe_uri(uri)
        );

        return;
    };

    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(rust_i18n::t!("external.title").to_string())
        .detail(shared::shorten_for_dialog(uri, 300))
        .buttons([
            rust_i18n::t!("dialog.cancel").to_string(),
            rust_i18n::t!("external.open").to_string(),
        ])
        .default_button(0)
        .cancel_button(0)
        .build();

    let uri = uri.to_string();

    dialog.choose(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            if matches!(result, Ok(1)) {
                launch_external(&uri);
            }
        },
    );
}

fn launch_external(uri: &str) {
    eprintln!(
        "Opening externally: {}",
        crate::browser::util::log_safe_uri(uri)
    );

    let failed = uri.to_string();
    let launcher = gtk::UriLauncher::new(uri);

    launcher.launch(
        None::<&gtk::Window>,
        None::<&gtk::gio::Cancellable>,
        move |result| {
            if let Err(error) = result {
                eprintln!(
                    "No application handles {}: {error}",
                    crate::browser::util::log_safe_uri(&failed)
                );
            }
        },
    );
}
