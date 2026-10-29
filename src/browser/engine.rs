use gtk::glib;
use gtk::glib::object::Cast;
use gtk::glib::translate::{FromGlibPtrFull as _, IntoGlib, ToGlibPtr as _};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webkit6::{
    AuthenticationRequest, HardwareAccelerationPolicy, LoadEvent, MemoryPressureSettings,
    NavigationPolicyDecision, NetworkSession, PermissionRequest, PermissionState,
    PolicyDecisionType, ResponsePolicyDecision, ScriptDialogType, WebProcessTerminationReason,
    WebView, prelude::*,
};

use super::permissions::{PermissionKind, PermissionManager};

pub struct BrowserEngine;

/// Handles WebKit's `authenticate` signal; the handler owns the request and
/// must eventually authenticate or cancel it.
pub type AuthenticateHandler = Rc<dyn Fn(&WebView, &AuthenticationRequest)>;

/// Which WebKit signal reported a popup request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PopupSource {
    /// The view's `create` signal (window.open()).
    Create,
    /// A `NewWindowAction` policy decision (target=_blank, window.open()).
    Policy,
}

/// Recently seen popups: URL, reporting signal and when. A list (not a
/// single slot) so interleaved `window.open()` calls that are reported as
/// `create(A), create(B), policy(A), policy(B)` still pair up correctly.
type PopupGuard = Rc<RefCell<Vec<(String, PopupSource, Instant)>>>;

fn new_popup_guard() -> PopupGuard {
    Rc::new(RefCell::new(Vec::new()))
}

/// Directory holding the HTTP disk cache. Only cache files live here —
/// website data (IndexedDB, local storage, …) lives next to it, so wiping
/// this directory is safe while the browser runs.
pub fn web_cache_dir() -> PathBuf {
    crate::paths::cache_dir().join("web-cache")
}

/// Normalizes a cookie-policy setting value and applies it to a session.
/// Unknown values fall back to blocking third-party cookies.
pub fn apply_cookie_policy(session: &NetworkSession, policy: &str) {
    use webkit6::CookieAcceptPolicy;

    let normalized = match policy {
        "always" => "always",
        "never" => "never",
        _ => "no-third-party",
    };

    let accept = match normalized {
        "always" => CookieAcceptPolicy::Always,
        "never" => CookieAcceptPolicy::Never,
        _ => CookieAcceptPolicy::NoThirdParty,
    };

    if let Some(cookie_manager) = session.cookie_manager() {
        cookie_manager.set_accept_policy(accept);
    }
}
/// Chrome-like UA so sites that gate on "Chromium-only" (Discord, Meet, Slack…)
/// serve their standard code instead of a broken fallback that OOMs the
/// WebProcess. Keeps the real WebKit version visible for diagnostics.
pub fn chrome_user_agent() -> String {
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36".to_string()
}

fn ensure_dirs() -> (String, String) {
    let data = crate::paths::data_dir();

    let website_data = data.join("web-data");
    let web_cache = web_cache_dir();

    for dir in [&website_data, &web_cache] {
        if let Err(error) = std::fs::create_dir_all(dir) {
            eprintln!(
                "Failed to create browser storage dir {}: {error}",
                dir.display()
            );
        }
    }

    (
        website_data.to_string_lossy().into_owned(),
        web_cache.to_string_lossy().into_owned(),
    )
}

fn tune_memory_pressure() {
    // Discord / Slack / Figma easily exceed WebKit's default web-process
    // memory limit and get killed with ExceededMemoryLimit. Raise the ceiling
    // globally (once per process is enough; the call is process-wide).
    let mut settings = MemoryPressureSettings::new();

    settings.set_memory_limit(4096);
    settings.set_poll_interval(5.0);

    NetworkSession::set_memory_pressure_settings(&mut settings);
}

impl BrowserEngine {
    /// Persistent session: explicit data/cache dirs so IndexedDB,
    /// ServiceWorkers and CacheStorage (which Discord relies on) survive and
    /// don't live in a temp/ram-backed store that blows the memory limit.
    pub fn create_network_session(itp_enabled: bool) -> NetworkSession {
        let (data, cache) = ensure_dirs();

        tune_memory_pressure();

        let session = NetworkSession::new(Some(&data), Some(&cache));

        session.set_persistent_credential_storage_enabled(true);
        session.set_itp_enabled(itp_enabled);

        session
    }

    /// Isolated session for private windows: nothing is persisted, but the
    /// same raised memory ceiling applies so heavy pages behave identically.
    pub fn create_ephemeral_session() -> NetworkSession {
        tune_memory_pressure();

        let session = NetworkSession::new_ephemeral();

        session.set_itp_enabled(true);
        session.set_persistent_credential_storage_enabled(false);

        apply_cookie_policy(&session, "no-third-party");

        session
    }

    /// Applies the user's GPU choice to a settings object. Used for new
    /// views and again for every open view when the setting changes.
    pub fn apply_hardware_acceleration(settings: &webkit6::Settings, enabled: bool) {
        settings.set_hardware_acceleration_policy(if enabled {
            HardwareAccelerationPolicy::Always
        } else {
            HardwareAccelerationPolicy::Never
        });
    }

    pub fn configure(view: &WebView, developer_tools: bool, hardware_acceleration: bool) {
        let Some(settings) = webkit6::prelude::WebViewExt::settings(view) else {
            return;
        };

        settings.set_enable_developer_extras(developer_tools);
        settings.set_enable_javascript(true);
        settings.set_enable_html5_local_storage(true);
        settings.set_enable_html5_database(true);

        // Compatibility shims without which complex SPAs misbehave.
        settings.set_enable_site_specific_quirks(true);
        settings.set_enable_page_cache(true);
        settings.set_enable_smooth_scrolling(true);

        // Media / realtime stack needed by Discord & friends.
        settings.set_enable_media(true);
        settings.set_enable_mediasource(true);
        settings.set_enable_media_capabilities(true);
        settings.set_enable_media_stream(true);
        settings.set_enable_webrtc(true);
        settings.set_enable_encrypted_media(true);
        settings.set_enable_webaudio(true);
        settings.set_enable_webgl(true);
        settings.set_enable_2d_canvas_acceleration(true);
        settings.set_enable_fullscreen(true);
        settings.set_media_playback_allows_inline(true);

        // Autoplay with sound needs a user gesture (Chromium parity).
        // WebRTC calls are unaffected: they use explicit permission grants.
        settings.set_media_playback_requires_user_gesture(true);

        // Popups (OAuth) get their own tab (see `wire_popups`).
        settings.set_javascript_can_open_windows_automatically(true);

        // Chromium parity: script-driven clipboard *writes* need a user
        // gesture. WebKit treats this as "no gesture needed at all" for both
        // `document.execCommand('copy'/'cut')` and `navigator.clipboard`
        // writes, which would let any page (background tabs included)
        // silently replace the clipboard. Reads never depend on it: they go
        // through the clipboard permission prompt instead.
        settings.set_javascript_can_access_clipboard(false);

        // Configurable (Settings → Appearance): forced acceleration breaks
        // systems without a working GPU (notably some WSLg setups).
        Self::apply_hardware_acceleration(&settings, hardware_acceleration);

        settings.set_user_agent(Some(chrome_user_agent()).as_deref());
    }

    /// Connects every WebKit signal a complex page needs handled. Without
    /// these, permission prompts never resolve, window.open() popups are
    /// dropped, script dialogs hang, and a dead web process leaves a blank
    /// tab that looks like a full browser crash.
    ///
    /// `open_in_new_tab` is called (via idle, to avoid re-entrancy) with the
    /// URL of popups / new-window navigations. `show_notification` receives
    /// (title, body) of web notifications to surface as desktop notifications.
    /// `on_authenticate` gets HTTP/proxy authentication requests; it must
    /// eventually call `authenticate()` or `cancel()` on the request.
    pub fn wire_view(
        view: &WebView,
        open_in_new_tab: Rc<dyn Fn(String)>,
        show_notification: Rc<dyn Fn(String, String)>,
        on_authenticate: AuthenticateHandler,
        permissions: PermissionManager,
        clipboard_reads: Rc<dyn Fn() -> bool>,
    ) {
        // Shared popup guard: window.open() typically fires `create` AND a
        // NewWindow policy decision for the same URL — one guard for both
        // paths so the same URL can never open two tabs.
        let popup_guard = new_popup_guard();

        Self::wire_policy(view, open_in_new_tab.clone(), popup_guard.clone());
        Self::wire_popups(view, open_in_new_tab, popup_guard);
        Self::wire_permissions(view, permissions, clipboard_reads);
        Self::wire_dialogs(view);
        Self::wire_crash_recovery(view);
        Self::wire_notifications(view, show_notification);
        Self::wire_authentication(view, on_authenticate);
    }

    /// Schemes the engine never loads itself: handed to the desktop
    /// instead (mail clients, dialers, …). Everything else WebKit owns:
    /// http(s), files, internal pages and script/data URLs.
    fn is_external_scheme(uri: &str) -> bool {
        let lower = uri.to_ascii_lowercase();

        let Some(colon) = lower.find(':') else {
            return false;
        };

        // `://foo` or `:foo`: no scheme at all — not something to hand to
        // the desktop.
        if colon == 0 {
            return false;
        }

        if lower[..colon].contains('/') {
            return false;
        }

        !matches!(
            &lower[..colon],
            "http" | "https" | "file" | "axys" | "about" | "blank" | "data" | "blob" | "javascript"
        )
    }

    /// First-seen check for a popup URL; records it when fresh.
    ///
    /// window.open() can report the same request through BOTH `create` and
    /// a NewWindow policy decision, with no shared request identity. So a
    /// request is a duplicate only when the same URL arrives through the
    /// *other* signal within a short window; that match is consumed. The
    /// same signal repeating a URL (a page deliberately opening it twice,
    /// OAuth retries) is always a genuine new opening and goes through.
    fn popup_is_fresh(guard: &PopupGuard, uri: &str, source: PopupSource) -> bool {
        let mut entries = guard.borrow_mut();

        let now = Instant::now();

        entries.retain(|(_, _, seen)| now.duration_since(*seen) <= Duration::from_secs(1));

        // A duplicate is the same URL arriving through the *other* signal;
        // consume exactly one matching entry so paired `create`+`policy`
        // emissions open a single tab even when several popups interleave.
        if let Some(position) = entries
            .iter()
            .position(|(seen, seen_source, _)| seen == uri && *seen_source != source)
        {
            entries.remove(position);

            false
        } else {
            entries.push((uri.to_string(), source, now));

            true
        }
    }

    fn wire_policy(view: &WebView, open_in_new_tab: Rc<dyn Fn(String)>, popup_guard: PopupGuard) {
        // Dedupe: window.open() typically fires `create` AND a NewWindow
        // policy decision for the same URL. Opening both would duplicate tabs.
        view.connect_decide_policy(move |_, decision, decision_type| {
            match decision_type {
                PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction => {
                    let navigation_action = decision
                        .downcast_ref::<NavigationPolicyDecision>()
                        .and_then(|d| d.navigation_action());

                    let uri = navigation_action
                        .as_ref()
                        .and_then(|a| a.request())
                        .and_then(|r| r.uri())
                        .map(|u| u.to_string());

                    // External schemes (mailto:, tel:, …) open in their
                    // desktop app instead of showing a WebKit error page.
                    // A user gesture is required: otherwise a hostile page
                    // could spam the OS protocol handler on load.
                    if let Some(uri) = uri.as_ref()
                        && Self::is_external_scheme(uri)
                    {
                        let gesture = navigation_action
                            .as_ref()
                            .is_some_and(|action| action.is_user_gesture());

                        if gesture {
                            eprintln!("Opening externally: {uri}");

                            if webkit6::gio::AppInfo::launch_default_for_uri(
                                uri,
                                None::<&webkit6::gio::AppLaunchContext>,
                            )
                            .is_err()
                            {
                                eprintln!("No application handles: {uri}");
                            }
                        } else {
                            eprintln!("Ignored external scheme without user gesture: {uri}");
                        }

                        decision.ignore();

                        return true;
                    }

                    if decision_type == PolicyDecisionType::NewWindowAction {
                        if let Some(uri) = uri
                            && (uri.starts_with("http://") || uri.starts_with("https://"))
                            && Self::popup_is_fresh(&popup_guard, &uri, PopupSource::Policy)
                        {
                            let opener = open_in_new_tab.clone();

                            glib::idle_add_local_once(move || opener(uri));
                        }

                        decision.ignore();

                        return true;
                    }

                    decision.use_();
                    true
                }
                PolicyDecisionType::Response => {
                    let supported = decision
                        .downcast_ref::<ResponsePolicyDecision>()
                        .map(|d| d.is_mime_type_supported())
                        .unwrap_or(true);

                    if supported {
                        decision.use_();
                    } else {
                        // e.g. Discord file / blob downloads: hand to the
                        // DownloadManager instead of stalling the page.
                        decision.download();
                    }

                    true
                }
                _ => {
                    decision.use_();
                    true
                }
            }
        });
    }

    fn wire_popups(view: &WebView, open_in_new_tab: Rc<dyn Fn(String)>, popup_guard: PopupGuard) {
        view.connect_create(move |_, action| {
            if let Some(uri) = action
                .request()
                .and_then(|r| r.uri())
                .map(|u| u.to_string())
                && (uri.starts_with("http://") || uri.starts_with("https://"))
                && Self::popup_is_fresh(&popup_guard, &uri, PopupSource::Create)
            {
                let opener = open_in_new_tab.clone();

                glib::idle_add_local_once(move || opener(uri));
            }

            // Return None: the URL is opened as a regular tab (see above)
            // instead of a half-wired related view, which is the stable
            // option for OAuth / window.open flows.
            None
        });
    }

    /// Full site origin for permission prompts: scheme + host + effective
    /// port. A grant for `https://example.com:443` must never apply to
    /// `http://example.com` or another port, so the bare host is not
    /// enough. IPv6 hosts get brackets. (Decisions stored by older
    /// versions under the bare host simply stop matching, which fails
    /// safe back to asking.)
    ///
    /// NOTE: this is the top-level page origin. WebKit's permission API
    /// does not reveal which embedded frame asked, so a prompt triggered
    /// by an iframe is attributed to the top page — treat unexpected
    /// prompts on unfamiliar pages as a reason to deny.
    fn permission_origin(view: &WebView) -> String {
        use super::permissions::GENERIC_ORIGIN;

        let parsed = view
            .uri()
            .and_then(|uri| gtk::glib::Uri::parse(&uri, gtk::glib::UriFlags::NONE).ok());

        let scheme = parsed
            .as_ref()
            .map(|uri| uri.scheme().to_string())
            .filter(|scheme| !scheme.is_empty())
            .unwrap_or_else(|| "https".to_string());

        let Some(host) = parsed
            .as_ref()
            .and_then(|uri| uri.host().map(|h| h.to_ascii_lowercase()))
            .map(|host| host.trim_end_matches('.').to_string())
            .filter(|host| !host.is_empty())
        else {
            return GENERIC_ORIGIN.to_string();
        };

        let port = parsed.as_ref().map(|uri| uri.port()).unwrap_or(-1);

        let effective = if port > 0 {
            port
        } else if scheme == "http" {
            80
        } else {
            443
        };

        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };

        format!("{scheme}://{host}:{effective}")
    }

    /// Permission prompt dialog. The WebKit request is answered
    /// asynchronously from the dialog buttons, so the page never hangs.
    fn ask_permission(
        view: &WebView,
        request: PermissionRequest,
        kind: PermissionKind,
        origin: String,
        permissions: PermissionManager,
    ) {
        let dialog = gtk::Window::builder()
            .modal(true)
            .title(rust_i18n::t!("permission.title").as_ref())
            .default_width(420)
            .build();

        if let Some(root) = view.root()
            && let Ok(parent) = root.downcast::<gtk::Window>()
        {
            dialog.set_transient_for(Some(&parent));
        }

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);

        content.set_margin_top(18);
        content.set_margin_bottom(18);
        content.set_margin_start(18);
        content.set_margin_end(18);

        let message = gtk::Label::new(Some(&rust_i18n::t!(
            "permission.message",
            host = origin,
            capability = rust_i18n::t!(kind.label_key()).as_ref()
        )));

        message.set_wrap(true);
        message.set_halign(gtk::Align::Start);

        let remember = gtk::CheckButton::with_label(&rust_i18n::t!("permission.remember"));

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);

        let block = gtk::Button::with_label(&rust_i18n::t!("permission.block"));
        let allow = gtk::Button::with_label(&rust_i18n::t!("permission.allow"));
        allow.add_css_class("suggested-action");

        buttons.append(&block);
        buttons.append(&allow);

        content.append(&message);
        content.append(&remember);
        content.append(&buttons);

        // Answered tracks whether Allow/Block ran: if the dialog is
        // dismissed any other way (window manager close), the request must
        // still be denied explicitly instead of left hanging.
        let answered = Rc::new(Cell::new(false));

        {
            // Weak: the button is a child of `content`/`dialog`, so a strong
            // capture here would make `dialog -> content -> button -> handler
            // -> dialog` a GObject refcount cycle that never finalizes.
            let dialog = dialog.downgrade();
            let permissions = permissions.clone();
            let origin = origin.clone();
            let remember = remember.clone();
            let request = request.clone();
            let answered = answered.clone();

            allow.connect_clicked(move |_| {
                answered.set(true);

                if remember.is_active() {
                    permissions.remember(&origin, kind, true);
                }

                request.allow();

                if let Some(dialog) = dialog.upgrade() {
                    dialog.close();
                }
            });
        }

        {
            let dialog = dialog.downgrade();
            let permissions = permissions.clone();
            let remember = remember.clone();
            let request = request.clone();
            let answered = answered.clone();

            block.connect_clicked(move |_| {
                answered.set(true);

                if remember.is_active() {
                    permissions.remember(&origin, kind, false);
                }

                request.deny();

                if let Some(dialog) = dialog.upgrade() {
                    dialog.close();
                }
            });
        }

        // If the tab/window is destroyed while the prompt is open, the
        // modal must not outlive it: close it (which denies the request).
        let destroy_handler: Rc<RefCell<Option<gtk::glib::SignalHandlerId>>> =
            Rc::new(RefCell::new(None));

        {
            let dialog_weak = dialog.downgrade();

            let id = view.connect_destroy(move |_| {
                if let Some(dialog) = dialog_weak.upgrade() {
                    dialog.close();
                }
            });

            *destroy_handler.borrow_mut() = Some(id);
        }

        {
            let request = request.clone();
            let answered = answered.clone();
            let view = view.clone();
            let destroy_handler = destroy_handler.clone();

            dialog.connect_close_request(move |_| {
                if !answered.get() {
                    answered.set(true);
                    request.deny();
                }

                if let Some(id) = destroy_handler.borrow_mut().take() {
                    view.disconnect(id);
                }

                gtk::glib::Propagation::Proceed
            });
        }

        dialog.set_child(Some(&content));
        dialog.present();
    }

    fn wire_permissions(
        view: &WebView,
        permissions: PermissionManager,
        clipboard_reads: Rc<dyn Fn() -> bool>,
    ) {
        view.connect_permission_request({
            let permissions = permissions.clone();
            let clipboard_reads = clipboard_reads.clone();

            move |view, request: &PermissionRequest| {
                // PermissionRequest is an interface; the concrete type name
                // tells us what is being asked for.
                let type_name = request.type_().name();

                let (is_display, is_audio, is_video) =
                    match request.downcast_ref::<webkit6::UserMediaPermissionRequest>() {
                        Some(media) => (
                            webkit6::functions::user_media_permission_is_for_display_device(media),
                            media.is_for_audio_device(),
                            media.is_for_video_device(),
                        ),
                        None => (false, false, false),
                    };

                let kind =
                    PermissionKind::classify_request(type_name, is_display, is_audio, is_video);

                if !kind.needs_prompt() {
                    match kind {
                        // Harmless and games need it: granted silently.
                        PermissionKind::PointerLock => request.allow(),
                        // Unknown kinds stay denied so the JS promise
                        // settles instead of hanging the page forever.
                        _ => {
                            eprintln!("Denied unknown web permission: {}", request.type_().name());
                            request.deny();
                        }
                    }

                    return true;
                }

                let origin = Self::permission_origin(view);

                if let Some(allowed) = permissions.remembered(&origin, kind) {
                    if allowed {
                        request.allow();
                    } else {
                        request.deny();
                    }

                    return true;
                }

                // Settings → Privacy: clipboard reads need no per-site prompt
                // while enabled. WebKit still requires transient user
                // activation and a secure context, and writes stay
                // gesture-gated (`javascript_can_access_clipboard(false)`).
                if kind == PermissionKind::Clipboard && clipboard_reads() {
                    request.allow();

                    return true;
                }

                Self::ask_permission(view, request.clone(), kind, origin, permissions.clone());

                true
            }
        });

        view.connect_query_permission_state({
            let permissions = permissions.clone();
            let clipboard_reads = clipboard_reads.clone();

            move |view, query| {
                // Report a remembered verdict so sites see the real state
                // instead of always being told "prompt". Kinds we never
                // remember (or the hostless generic origin) stay Prompt so
                // the page still asks.
                let kind = query
                    .name()
                    .as_deref()
                    .map(PermissionKind::classify)
                    .unwrap_or(PermissionKind::Other);

                let origin = Self::permission_origin(view);

                let state = match permissions.remembered(&origin, kind) {
                    Some(true) => PermissionState::Granted,
                    Some(false) => PermissionState::Denied,
                    None if kind == PermissionKind::Clipboard && clipboard_reads() => {
                        PermissionState::Granted
                    }
                    None => PermissionState::Prompt,
                };

                query.finish(state);

                true
            }
        });
    }

    fn wire_dialogs(view: &WebView) {
        view.connect_script_dialog(|view, dialog| {
            // alert()/confirm() get a real, non-blocking dialog attached to
            // the window (the old auto-answer made every "Are you sure?"
            // fail). prompt()/beforeunload stay auto-handled so a page can
            // never trap the tab in a modal loop that looks like a hang.
            match dialog.dialog_type() {
                ScriptDialogType::Alert | ScriptDialogType::Confirm => {
                    let is_confirm = dialog.dialog_type() == ScriptDialogType::Confirm;

                    let Some(parent) = view.root().and_downcast::<gtk::Window>() else {
                        // No window to attach to: fall back to the old behaviour.
                        dialog.confirm_set_confirmed(false);
                        dialog.close();

                        return true;
                    };

                    let host = view
                        .uri()
                        .and_then(|uri| gtk::glib::Uri::parse(&uri, gtk::glib::UriFlags::NONE).ok())
                        .and_then(|uri| uri.host().map(|host| host.to_string()))
                        .unwrap_or_default();

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
                        .message(dialog.message().map(|m| m.to_string()).unwrap_or_default())
                        .detail(host)
                        .buttons(buttons)
                        .default_button(ok_index)
                        .cancel_button(if is_confirm { 0 } else { ok_index })
                        .build();

                    let dialog = dialog.clone();

                    alert.choose(
                        Some(&parent),
                        None::<&gtk::gio::Cancellable>,
                        move |result| {
                            if is_confirm {
                                dialog.confirm_set_confirmed(
                                    matches!(result, Ok(index) if index == ok_index),
                                );
                            }

                            dialog.close();
                        },
                    );
                }
                ScriptDialogType::Prompt => {
                    let fallback = dialog
                        .prompt_get_default_text()
                        .map(|t| t.to_string())
                        .unwrap_or_default();

                    dialog.prompt_set_text(&fallback);
                    dialog.confirm_set_confirmed(false);
                    dialog.close();
                }
                ScriptDialogType::BeforeUnloadConfirm => {
                    // true = let the navigation proceed (never trap an SPA).
                    dialog.confirm_set_confirmed(true);
                    dialog.close();
                }
                _ => {
                    dialog.close();
                }
            }

            true
        });
    }

    fn error_page(title: &str, detail: &str, uri: &str) -> String {
        let esc = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        };

        let hint = rust_i18n::t!("error.hint").to_string();

        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <meta name=\"color-scheme\" content=\"light dark\">\
             <style>:root{{color-scheme:light dark}}\
             body{{font-family:system-ui,sans-serif;background:Canvas;color:CanvasText;\
             display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0}}\
             .card{{max-width:560px;padding:32px;background:rgba(127,127,127,0.12);\
             border-radius:12px}}\
             h1{{font-size:20px}}code{{font-weight:600;word-break:break-all}}\
             p{{line-height:1.5}}</style></head><body><div class=\"card\">\
             <h1>{}</h1><p>{}</p><p><code>{}</code></p>\
             <p>{}</p>\
             </div></body></html>",
            esc(title),
            esc(detail),
            esc(uri),
            esc(&hint)
        )
    }

    /// Shows the generic load-failure page for a locally detected timeout
    /// (used by the navigation watchdog; WebKit itself has no load timeout).
    pub fn show_timeout_page(view: &WebView, uri: &str) {
        let title = rust_i18n::t!("error.load_title").to_string();
        let detail = rust_i18n::t!("error.timeout_detail").to_string();

        Self::mark_error_page(view, uri);

        view.load_alternate_html(&Self::error_page(&title, &detail, uri), uri, None);
    }

    /// Records, on the view, the URI whose load failed and for which an error
    /// page is now shown, so history recording can skip it.
    fn mark_error_page(view: &WebView, uri: &str) {
        const ERROR_PAGE_URI_KEY: &str = "axys-error-page-uri";

        unsafe {
            view.set_data(ERROR_PAGE_URI_KEY, uri.to_string());
        }
    }

    /// Returns true (and clears the mark) when `uri` is the URI of an error
    /// page currently shown in `view`. Successful loads of the same URI later
    /// are recorded normally because the mark is consumed here.
    pub fn take_error_page(view: &WebView, uri: &str) -> bool {
        const ERROR_PAGE_URI_KEY: &str = "axys-error-page-uri";

        let marked = unsafe {
            view.data::<String>(ERROR_PAGE_URI_KEY)
                .map(|stored| stored.as_ref().as_str() == uri)
                .unwrap_or(false)
        };

        if marked {
            unsafe {
                view.steal_data::<String>(ERROR_PAGE_URI_KEY);
            }
        }

        marked
    }

    /// Removes persistent website data (IndexedDB, local storage, DOM
    /// caches, …) of the given types from `manager`. The safe bindings omit
    /// `webkit_website_data_manager_clear`, so this calls the FFI directly.
    /// `on_done` reports success; pass `None` to only log failures.
    pub fn clear_website_data(
        manager: &webkit6::WebsiteDataManager,
        types: webkit6::WebsiteDataTypes,
        on_done: Option<Rc<dyn Fn(bool)>>,
    ) {
        // Ten years in microseconds: effectively "everything ever stored".
        const TEN_YEARS_US: i64 = 10 * 365 * 24 * 3600 * 1_000_000;

        unsafe extern "C" fn finished(
            source: *mut gtk::glib::gobject_ffi::GObject,
            result: *mut webkit6::gio::ffi::GAsyncResult,
            user_data: gtk::glib::ffi::gpointer,
        ) {
            let mut error = std::ptr::null_mut();

            let ok = unsafe {
                webkit6_sys::webkit_website_data_manager_clear_finish(
                    source as *mut _,
                    result,
                    &mut error,
                )
            } != 0;

            if !ok {
                let message = if error.is_null() {
                    "unknown error".to_string()
                } else {
                    unsafe { gtk::glib::Error::from_glib_full(error).to_string() }
                };

                eprintln!("Failed to clear website data: {message}");
            }

            if !user_data.is_null() {
                let callback: Box<Rc<dyn Fn(bool)>> = unsafe { Box::from_raw(user_data as *mut _) };

                callback(ok);
            }
        }

        let user_data = match on_done {
            Some(callback) => Box::into_raw(Box::new(callback)) as *mut std::ffi::c_void,
            None => std::ptr::null_mut(),
        };

        unsafe {
            webkit6_sys::webkit_website_data_manager_clear(
                manager.to_glib_none().0,
                types.into_glib(),
                TEN_YEARS_US,
                std::ptr::null_mut(),
                Some(finished),
                user_data,
            );
        }
    }

    fn wire_crash_recovery(view: &WebView) {
        view.connect_web_process_terminated(|view, reason| {
            let uri = view.uri().map(|u| u.to_string()).unwrap_or_default();

            // TerminatedByApi is us (or an extension) stopping the process on
            // purpose — not a crash, don't clobber the page.
            if reason == WebProcessTerminationReason::TerminatedByApi {
                return;
            }

            let (title, detail) = match reason {
                WebProcessTerminationReason::ExceededMemoryLimit => (
                    rust_i18n::t!("error.memory_title").to_string(),
                    rust_i18n::t!("error.memory_detail").to_string(),
                ),
                _ => (
                    rust_i18n::t!("error.crashed_title").to_string(),
                    rust_i18n::t!("error.crashed_detail").to_string(),
                ),
            };

            eprintln!("Web process terminated ({reason:?}) on {uri}");

            Self::mark_error_page(view, &uri);

            view.load_alternate_html(&Self::error_page(&title, &detail, &uri), &uri, None);
        });

        view.connect_load_failed(|view, event, failing_uri, error| {
            let message = error.message().to_string();
            let lower = message.to_ascii_lowercase();

            // Policy-change interruptions and user cancellations are part of
            // normal SPA navigation — showing an error page would break them.
            if lower.contains("interrupt") || lower.contains("cancell") || lower.contains("policy")
            {
                return false;
            }

            if event == LoadEvent::Committed && failing_uri.is_empty() {
                return false;
            }

            eprintln!("Load failed ({event:?}) {failing_uri}: {message}");

            let title = rust_i18n::t!("error.load_title").to_string();

            Self::mark_error_page(view, failing_uri);

            view.load_alternate_html(
                &Self::error_page(&title, &message, failing_uri),
                failing_uri,
                None,
            );

            true
        });

        view.connect_load_failed_with_tls_errors(|view, failing_uri, _cert, _errors| {
            eprintln!("TLS failure on {failing_uri}");

            let title = rust_i18n::t!("error.tls_title").to_string();
            let detail = rust_i18n::t!("error.tls_detail").to_string();

            Self::mark_error_page(view, failing_uri);

            view.load_alternate_html(
                &Self::error_page(&title, &detail, failing_uri),
                failing_uri,
                None,
            );

            true
        });
    }

    fn wire_notifications(view: &WebView, show_notification: Rc<dyn Fn(String, String)>) {
        view.connect_show_notification(move |_, notification| {
            let title = notification
                .title()
                .map(|title| title.to_string())
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| rust_i18n::t!("notification.untitled").to_string());

            let body = notification
                .body()
                .map(|body| body.to_string())
                .unwrap_or_default();

            show_notification(title, body);

            // Dismiss the web-side notification; the desktop one replaces it.
            notification.close();

            true
        });
    }

    fn wire_authentication(view: &WebView, on_authenticate: AuthenticateHandler) {
        view.connect_authenticate(move |view, request| {
            // The dialog is asynchronous, so claim the signal to stop WebKit
            // from cancelling the request while the user types.
            on_authenticate(view, request);

            true
        });
    }

    pub fn load(view: &WebView, uri: &str) {
        view.load_uri(uri);
    }

    pub fn reload(view: &WebView) {
        // If the web process is gone, reload() respawns it; bypass-cache
        // would do the same but slower, so plain reload is the right default.
        view.reload();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "https://example.com/a";

    #[test]
    fn popup_guard_dedupes_create_plus_policy_pair() {
        let guard = new_popup_guard();

        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));
        assert!(!BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Policy
        ));
    }

    #[test]
    fn popup_guard_dedupes_in_either_order() {
        let guard = new_popup_guard();

        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Policy
        ));
        assert!(!BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));
    }

    #[test]
    fn popup_guard_allows_same_url_twice_from_one_signal() {
        // Two real window.open(A) calls must open two tabs.
        let guard = new_popup_guard();

        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));
        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));
    }

    #[test]
    fn popup_guard_allows_different_urls() {
        let guard = new_popup_guard();

        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));
        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            "https://example.com/b",
            PopupSource::Policy
        ));
    }

    #[test]
    fn popup_guard_match_is_consumed() {
        let guard = new_popup_guard();

        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));
        assert!(!BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Policy
        ));
        // A later genuine opening of the same URL still works.
        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Policy
        ));
    }

    #[test]
    fn popup_guard_expires() {
        let guard = new_popup_guard();

        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));

        let stale = Instant::now()
            .checked_sub(Duration::from_secs(5))
            .unwrap_or_else(Instant::now);

        for entry in guard.borrow_mut().iter_mut() {
            entry.2 = stale;
        }

        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Policy
        ));
    }

    #[test]
    fn popup_guard_pairs_interleaved_popups() {
        // Two window.open() calls reported as create(A), create(B),
        // policy(A), policy(B) must yield exactly two openings, not four.
        let guard = new_popup_guard();

        const B: &str = "https://example.com/b";

        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));
        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            B,
            PopupSource::Create
        ));
        assert!(!BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Policy
        ));
        assert!(!BrowserEngine::popup_is_fresh(
            &guard,
            B,
            PopupSource::Policy
        ));
    }

    #[test]
    fn popup_guard_pairs_duplicate_ur_ls() {
        // create(A), create(A), policy(A), policy(A) = two real openings.
        let guard = new_popup_guard();

        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));
        assert!(BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Create
        ));
        assert!(!BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Policy
        ));
        assert!(!BrowserEngine::popup_is_fresh(
            &guard,
            A,
            PopupSource::Policy
        ));
    }
}
