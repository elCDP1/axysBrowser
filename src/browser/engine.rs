use gtk::glib;
use gtk::glib::object::Cast;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webkit6::{
    HardwareAccelerationPolicy, LoadEvent, MemoryPressureSettings, NavigationPolicyDecision,
    NetworkSession, PermissionRequest, PermissionState, PolicyDecisionType, ResponsePolicyDecision,
    ScriptDialogType, WebProcessTerminationReason, WebView, prelude::*,
};

pub struct BrowserEngine;

fn data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("axysbrowser");
    }

    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("axysbrowser");
    }

    PathBuf::from("axysbrowser-data")
}

fn cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("axysbrowser");
    }

    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".cache").join("axysbrowser");
    }

    PathBuf::from("axysbrowser-cache")
}

/// Directory holding the HTTP disk cache. Only cache files live here —
/// website data (IndexedDB, local storage, …) lives next to it, so wiping
/// this directory is safe while the browser runs.
pub fn web_cache_dir() -> PathBuf {
    cache_dir().join("web-cache")
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
    let data = data_dir();

    let website_data = data.join("web-data");
    let web_cache = web_cache_dir();

    for dir in [&website_data, &web_cache] {
        if let Err(error) = std::fs::create_dir_all(dir) {
            eprintln!("Failed to create browser storage dir {}: {error}", dir.display());
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

    pub fn configure(view: &WebView, developer_tools: bool) {
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
        settings.set_media_playback_requires_user_gesture(false);

        // Popups (OAuth) and rendering.
        settings.set_javascript_can_open_windows_automatically(true);
        settings.set_javascript_can_access_clipboard(true);
        settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Always);

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
    pub fn wire_view(
        view: &WebView,
        open_in_new_tab: Rc<dyn Fn(String)>,
        show_notification: Rc<dyn Fn(String, String)>,
    ) {
        Self::wire_policy(view, open_in_new_tab.clone());
        Self::wire_popups(view, open_in_new_tab);
        Self::wire_permissions(view);
        Self::wire_dialogs(view);
        Self::wire_crash_recovery(view);
        Self::wire_notifications(view, show_notification);
    }

    fn wire_policy(view: &WebView, open_in_new_tab: Rc<dyn Fn(String)>) {
        // Dedupe: window.open() typically fires `create` AND a NewWindow
        // policy decision for the same URL. Opening both would duplicate tabs.
        let last_popup: Rc<RefCell<(String, Instant)>> = Rc::new(RefCell::new((
            String::new(),
            Instant::now()
                .checked_sub(Duration::from_secs(10))
                .unwrap_or_else(Instant::now),
        )));

        view.connect_decide_policy(move |_, decision, decision_type| {
            match decision_type {
                PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction => {
                    let uri = decision
                        .downcast_ref::<NavigationPolicyDecision>()
                        .and_then(|d| d.navigation_action())
                        .and_then(|a| a.request())
                        .and_then(|r| r.uri())
                        .map(|u| u.to_string());

                    if decision_type == PolicyDecisionType::NewWindowAction {
                        if let Some(uri) = uri
                            && (uri.starts_with("http://") || uri.starts_with("https://"))
                        {
                            let fresh = {
                                let last = last_popup.borrow();

                                last.0 != uri || last.1.elapsed() > Duration::from_secs(1)
                            };

                            if fresh {
                                *last_popup.borrow_mut() =
                                    (uri.clone(), Instant::now());

                                let opener = open_in_new_tab.clone();

                                glib::idle_add_local_once(move || opener(uri));
                            }

                            decision.ignore();
                        } else {
                            decision.ignore();
                        }

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

    fn wire_popups(view: &WebView, open_in_new_tab: Rc<dyn Fn(String)>) {
        let last_popup: Rc<RefCell<(String, Instant)>> = Rc::new(RefCell::new((
            String::new(),
            Instant::now()
                .checked_sub(Duration::from_secs(10))
                .unwrap_or_else(Instant::now),
        )));

        view.connect_create(move |_, action| {
            if let Some(uri) = action
                .request()
                .and_then(|r| r.uri())
                .map(|u| u.to_string())
                && (uri.starts_with("http://") || uri.starts_with("https://"))
            {
                let fresh = {
                    let last = last_popup.borrow();

                    last.0 != uri || last.1.elapsed() > Duration::from_secs(1)
                };

                if fresh {
                    *last_popup.borrow_mut() = (uri.clone(), Instant::now());

                    let opener = open_in_new_tab.clone();

                    glib::idle_add_local_once(move || opener(uri));
                }
            }

            // Return None: the URL is opened as a regular tab (see above)
            // instead of a half-wired related view, which is the stable
            // option for OAuth / window.open flows.
            None
        });
    }

    fn wire_permissions(view: &WebView) {
        view.connect_permission_request(|_, request: &PermissionRequest| {
            // PermissionRequest is an interface; the concrete type name tells
            // us what is being asked for.
            let kind = request.type_().name().to_string();
            let lower = kind.to_ascii_lowercase();

            if lower.contains("notification") {
                request.allow();
            } else {
                // Mic / camera / display capture / geolocation / DRM, …:
                // deny by default so the JS promise always settles instead of
                // hanging the page (and growing memory) forever. Voice/video
                // then fails gracefully instead of killing the web process.
                eprintln!("Denied web permission: {kind}");
                request.deny();
            }

            true
        });

        view.connect_query_permission_state(|_, query| {
            let name = query.name().map(|n| n.to_string()).unwrap_or_default();

            if name.to_ascii_lowercase().contains("notification") {
                query.finish(PermissionState::Granted);
            } else {
                query.finish(PermissionState::Denied);
            }

            true
        });
    }

    fn wire_dialogs(view: &WebView) {
        view.connect_script_dialog(|_, dialog| {
            // Auto-handle alert()/confirm()/prompt()/beforeunload so a page
            // can never trap the tab in a modal loop that looks like a hang.
            match dialog.dialog_type() {
                ScriptDialogType::Alert => {
                    dialog.close();
                }
                ScriptDialogType::Confirm => {
                    dialog.confirm_set_confirmed(false);
                    dialog.close();
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
             <style>body{{font-family:system-ui,sans-serif;background:#1e1e2e;color:#cdd6f4;\
             display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0}}\
             .card{{max-width:560px;padding:32px;background:#313244;border-radius:12px}}\
             h1{{font-size:20px}}code{{color:#f9e2af;word-break:break-all}}\
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

            view.load_alternate_html(
                &Self::error_page(&title, &detail, &uri),
                &uri,
                None,
            );
        });

        view.connect_load_failed(|view, event, failing_uri, error| {
            let message = error.message().to_string();
            let lower = message.to_ascii_lowercase();

            // Policy-change interruptions and user cancellations are part of
            // normal SPA navigation — showing an error page would break them.
            if lower.contains("interrupt")
                || lower.contains("cancell")
                || lower.contains("policy")
            {
                return false;
            }

            if event == LoadEvent::Committed && failing_uri.is_empty() {
                return false;
            }

            eprintln!("Load failed ({event:?}) {failing_uri}: {message}");

            let title = rust_i18n::t!("error.load_title").to_string();

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
                .unwrap_or_else(|| "Web notification".to_string());

            let body = notification
                .body()
                .map(|body| body.to_string())
                .unwrap_or_default();

            show_notification(title, body);

            // Dismiss the web-side notification; the desktop one replaces it.
            notification.close();

            true
        });

        view.connect_authenticate(|_, _request| {
            // No interactive HTTP-auth handler yet: let WebKit cancel the
            // request instead of hanging the load.
            false
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
