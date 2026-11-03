//! Per-origin website permission decisions.
//!
//! Replaces blanket auto-allow/auto-deny: the first time an origin asks for
//! something sensitive (notifications, camera/microphone, geolocation,
//! screen sharing, DRM, clipboard reads), the browser prompts and can
//! remember the answer.
//! Decisions persist as TOML next to the other profile data; private
//! windows use an ephemeral manager that never touches disk.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

/// What the page is asking for, classified from the WebKit request type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionKind {
    Notification,
    Geolocation,
    /// Camera only (`getUserMedia({video: true})`).
    Camera,
    /// Microphone only (`getUserMedia({audio: true})`).
    Microphone,
    /// Camera and microphone in a single WebKit user-media request. Stored
    /// as two separate decisions (Camera + Microphone) so a later single-
    /// device request never reuses the other one's grant.
    Media,
    ScreenShare,
    Drm,
    /// Pointer lock is harmless and games need it: granted silently.
    PointerLock,
    /// Clipboard reads (`navigator.clipboard.readText()`/`read()`), asked
    /// per origin and still gated by WebKit on a user gesture.
    Clipboard,
    /// Anything else: denied silently (status quo ante).
    Other,
}

impl PermissionKind {
    /// Classifies a WebKit permission-request type name.
    pub fn classify(type_name: &str) -> Self {
        let lower = type_name.to_ascii_lowercase();

        if lower.contains("notification") {
            Self::Notification
        } else if lower.contains("geolocation") || lower.contains("geoposition") {
            Self::Geolocation
        } else if lower.contains("usermedia")
            || lower.contains("camera")
            || lower.contains("microphone")
        {
            Self::Media
        } else if lower.contains("display") || lower.contains("screen") || lower.contains("capture")
        {
            Self::ScreenShare
        } else if lower.contains("mediakey") || lower.contains("keysystem") || lower.contains("drm")
        {
            Self::Drm
        } else if lower.contains("pointerlock") || lower.contains("pointer-lock") {
            Self::PointerLock
        } else if lower.contains("clipboard") {
            Self::Clipboard
        } else {
            Self::Other
        }
    }

    /// Like [`classify`](Self::classify) but aware of display capture and
    /// of which user-media devices are requested: `getDisplayMedia()`
    /// arrives as the very same `WebKitUserMediaPermissionRequest` type as
    /// camera/microphone, so the type name alone would file screen sharing
    /// under `Media` and let a remembered camera/mic grant silently allow
    /// screen capture. Camera and microphone are likewise split so a
    /// remembered audio-only grant can never authorize the camera.
    pub fn classify_request(
        type_name: &str,
        is_display_capture: bool,
        is_audio: bool,
        is_video: bool,
    ) -> Self {
        match Self::classify(type_name) {
            Self::Media if is_display_capture => Self::ScreenShare,
            Self::Media => match (is_audio, is_video) {
                (true, true) => Self::Media,
                (false, true) => Self::Camera,
                (true, false) => Self::Microphone,
                // Unknown media request: prompt as a combined one.
                (false, false) => Self::Media,
            },
            kind => kind,
        }
    }

    /// Whether this kind ever prompts (as opposed to an automatic verdict).
    pub fn needs_prompt(self) -> bool {
        matches!(
            self,
            Self::Notification
                | Self::Geolocation
                | Self::Camera
                | Self::Microphone
                | Self::Media
                | Self::ScreenShare
                | Self::Drm
                | Self::Clipboard
        )
    }

    /// Locale key suffix for the human-readable capability name.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Notification => "permission.use_notifications",
            Self::Geolocation => "permission.use_location",
            Self::Camera => "permission.use_camera",
            Self::Microphone => "permission.use_microphone",
            Self::Media => "permission.use_camera_mic",
            Self::ScreenShare => "permission.share_screen",
            Self::Drm => "permission.use_drm",
            Self::PointerLock => "permission.use_pointer_lock",
            Self::Clipboard => "permission.use_clipboard",
            Self::Other => "permission.use_other",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PermissionStore {
    /// origin host -> kind key -> allowed.
    decisions: HashMap<String, HashMap<String, bool>>,
}

fn kind_key(kind: PermissionKind) -> String {
    format!("{kind:?}").to_ascii_lowercase()
}

/// Fallback identity for pages without a real origin (about:blank, data:,
/// file errors). Never recorded and never consulted: every such page asks
/// afresh, so unrelated hostless pages cannot share grants.
pub const GENERIC_ORIGIN: &str = "this site";

/// Whether a clipboard read may skip the per-site prompt: only while the
/// setting is on **and** the origin is verifiable. Hostless pages (`data:`,
/// `about:`, `file:`, `blob:` …) share [`GENERIC_ORIGIN`], which always asks
/// and is never remembered; the setting must not widen that.
pub fn clipboard_read_without_prompt(origin: &str, setting_enabled: bool) -> bool {
    setting_enabled && origin != GENERIC_ORIGIN
}

/// Canonical origin used as the key for permission decisions: scheme, host
/// and effective port, so `https://example.com:443` can never share a grant
/// with `http://example.com`, another port or a look-alike host.
///
/// The host is lowercased and a trailing root dot dropped (DNS is
/// case-insensitive, so `Example.COM.` and `example.com` are one site), the
/// scheme is lowercased (the same site however it was spelled), and IPv6
/// literals keep exactly one pair of brackets. Only `http`/`https` pages with
/// a real host get an identity that can be remembered: hostless pages and
/// every non-web scheme (`about:`, `data:`, `blob:`, `file:` — even
/// `file://machine/share`, whose "host" is a machine name, not a web origin)
/// map to [`GENERIC_ORIGIN`], which is never stored and never consulted. A
/// local file therefore cannot inherit, or persist, a camera or notification
/// grant.
pub fn origin_for_uri(uri: Option<&str>) -> String {
    let Some(raw) = uri.map(str::trim).filter(|uri| !uri.is_empty()) else {
        return GENERIC_ORIGIN.to_string();
    };

    let Ok(parsed) = gtk::glib::Uri::parse(raw, gtk::glib::UriFlags::NONE) else {
        return GENERIC_ORIGIN.to_string();
    };

    let scheme = parsed.scheme().to_string().to_ascii_lowercase();

    if scheme != "http" && scheme != "https" {
        return GENERIC_ORIGIN.to_string();
    }

    let Some(host) = parsed
        .host()
        .map(|host| host.to_ascii_lowercase())
        .map(|host| host.trim_end_matches('.').to_string())
        .filter(|host| !host.is_empty())
    else {
        return GENERIC_ORIGIN.to_string();
    };

    // Idempotent bracketing: GLib may or may not include them for IPv6.
    let host = host.trim_start_matches('[').trim_end_matches(']');

    let port = parsed.port();

    // GLib reports `-1` when the URI carries no port at all. An explicit `:0`
    // is a port like any other (a service on an ephemeral port is still its
    // own origin), so only `-1` falls back to the scheme's default.
    let effective = if port >= 0 {
        port
    } else if scheme == "http" {
        80
    } else {
        443
    };

    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };

    format!("{scheme}://{host}:{effective}")
}

/// Remembers per-origin permission decisions, optionally on disk.
#[derive(Clone)]
pub struct PermissionManager {
    store: Rc<RefCell<PermissionStore>>,
    /// None = ephemeral (private windows): decide and prompt, never save.
    path: Option<PathBuf>,
}

impl PermissionManager {
    /// Loads (or starts) the persistent profile-wide manager.
    pub fn load() -> Self {
        let path = Self::storage_path();

        let mut store = fs::read_to_string(&path)
            .ok()
            .and_then(|contents| toml::from_str::<PermissionStore>(&contents).ok())
            .unwrap_or_default();

        let migrated = Self::migrate_legacy_media(&mut store);

        let manager = Self {
            store: Rc::new(RefCell::new(store)),
            path: Some(path),
        };

        // Persist immediately: otherwise a crash before the next decision
        // would leave the old `media` key on disk to migrate again.
        if migrated {
            manager.save();
        }

        manager
    }

    /// Older versions stored a single combined camera+microphone decision
    /// under the `media` key. Split it into the per-device keys the current
    /// code reads, so upgrading users don't get re-prompted for a grant they
    /// already gave.
    fn migrate_legacy_media(store: &mut PermissionStore) -> bool {
        let mut changed = false;

        for kinds in store.decisions.values_mut() {
            if let Some(allowed) = kinds.remove("media") {
                kinds.entry("camera".to_string()).or_insert(allowed);
                kinds.entry("microphone".to_string()).or_insert(allowed);

                changed = true;
            }
        }

        changed
    }

    /// Starts an empty manager that never touches disk.
    pub fn ephemeral() -> Self {
        Self {
            store: Rc::new(RefCell::new(PermissionStore::default())),
            path: None,
        }
    }

    /// A remembered verdict for this origin + kind, if any. The generic
    /// `"this site"` identity (hostless pages) is never consulted: it
    /// would otherwise let one such page reuse another's sensitive grant.
    ///
    /// A combined camera+microphone request (`Media`) is only remembered
    /// when *both* devices agree, so a camera-only grant cannot silently
    /// authorize a later request that also wants the microphone.
    pub fn remembered(&self, origin: &str, kind: PermissionKind) -> Option<bool> {
        if origin == GENERIC_ORIGIN {
            return None;
        }

        if kind == PermissionKind::Media {
            let camera = self.raw(origin, PermissionKind::Camera);
            let microphone = self.raw(origin, PermissionKind::Microphone);

            return match (camera, microphone) {
                (Some(camera), Some(microphone)) if camera == microphone => Some(camera),
                _ => None,
            };
        }

        self.raw(origin, kind)
    }

    fn raw(&self, origin: &str, kind: PermissionKind) -> Option<bool> {
        self.store
            .borrow()
            .decisions
            .get(origin)
            .and_then(|kinds| kinds.get(&kind_key(kind)))
            .copied()
    }

    /// Records a verdict (persisted unless ephemeral). The generic
    /// `"this site"` identity is never recorded at all: every hostless
    /// page asks afresh, so unrelated pages cannot share grants.
    ///
    /// A combined camera+microphone decision is stored as two separate
    /// entries so each device can later be revoked independently.
    pub fn remember(&self, origin: &str, kind: PermissionKind, allowed: bool) {
        if origin == GENERIC_ORIGIN {
            return;
        }

        {
            let mut store = self.store.borrow_mut();

            let kinds = store.decisions.entry(origin.to_string()).or_default();

            if kind == PermissionKind::Media {
                kinds.insert(kind_key(PermissionKind::Camera), allowed);
                kinds.insert(kind_key(PermissionKind::Microphone), allowed);
            } else {
                kinds.insert(kind_key(kind), allowed);
            }
        }

        self.save();
    }

    /// Forgets every remembered decision.
    pub fn clear_all(&self) {
        self.store.borrow_mut().decisions.clear();

        self.save();
    }

    /// How many (origin, kind) decisions are stored.
    pub fn decision_count(&self) -> usize {
        self.store
            .borrow()
            .decisions
            .values()
            .map(HashMap::len)
            .sum()
    }

    fn save(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };

        match toml::to_string_pretty(&*self.store.borrow()) {
            Ok(contents) => {
                if let Err(error) = crate::paths::atomic_write(path, &contents) {
                    eprintln!("Failed to save permissions: {error}");
                }
            }
            Err(error) => eprintln!("Failed to serialize permissions: {error}"),
        }
    }

    fn storage_path() -> PathBuf {
        crate::paths::config_dir().join("permissions.toml")
    }
}

/// Reads a TOML store from `path` (test helper); corrupt files yield empty.
#[cfg(test)]
fn load_store_for_test(path: &std::path::Path) -> Result<PermissionStore, std::io::Error> {
    let contents = fs::read_to_string(path)?;
    toml::from_str(&contents).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_request_types() {
        assert_eq!(
            PermissionKind::classify("WebKitNotificationPermissionRequest"),
            PermissionKind::Notification
        );
        assert_eq!(
            PermissionKind::classify("WebKitGeolocationPermissionRequest"),
            PermissionKind::Geolocation
        );
        assert_eq!(
            PermissionKind::classify("WebKitUserMediaPermissionRequest"),
            PermissionKind::Media
        );
        assert_eq!(
            PermissionKind::classify("WebKitPointerLockPermissionRequest"),
            PermissionKind::PointerLock
        );
        assert_eq!(
            PermissionKind::classify("WebKitClipboardPermissionRequest"),
            PermissionKind::Clipboard
        );
        assert_eq!(
            PermissionKind::classify("SomethingUnknown"),
            PermissionKind::Other
        );
    }

    #[test]
    fn display_capture_is_never_filed_as_camera_or_mic() {
        let name = "WebKitUserMediaPermissionRequest";

        assert_eq!(
            PermissionKind::classify_request(name, false, true, true),
            PermissionKind::Media
        );
        assert_eq!(
            PermissionKind::classify_request(name, true, true, true),
            PermissionKind::ScreenShare
        );
        // Single-device requests are split so grants cannot cross over.
        assert_eq!(
            PermissionKind::classify_request(name, false, false, true),
            PermissionKind::Camera
        );
        assert_eq!(
            PermissionKind::classify_request(name, false, true, false),
            PermissionKind::Microphone
        );
        // Other request types are unaffected by the flags.
        assert_eq!(
            PermissionKind::classify_request(
                "WebKitGeolocationPermissionRequest",
                true,
                true,
                true
            ),
            PermissionKind::Geolocation
        );
    }

    #[test]
    fn camera_grant_does_not_cover_screen_share() {
        let manager = PermissionManager::ephemeral();

        manager.remember("https://meet.example:443", PermissionKind::Media, true);

        assert_eq!(
            manager.remembered("https://meet.example:443", PermissionKind::ScreenShare),
            None
        );
    }

    #[test]
    fn camera_and_microphone_do_not_cross_grant() {
        let manager = PermissionManager::ephemeral();

        manager.remember("https://meet.example:443", PermissionKind::Camera, true);

        // The camera grant must not authorize the microphone…
        assert_eq!(
            manager.remembered("https://meet.example:443", PermissionKind::Microphone),
            None
        );

        // …nor a combined camera+microphone request.
        assert_eq!(
            manager.remembered("https://meet.example:443", PermissionKind::Media),
            None
        );

        assert_eq!(
            manager.remembered("https://meet.example:443", PermissionKind::Camera),
            Some(true)
        );
    }

    #[test]
    fn combined_media_grant_covers_both_devices() {
        let manager = PermissionManager::ephemeral();

        manager.remember("https://meet.example:443", PermissionKind::Media, true);

        assert_eq!(
            manager.remembered("https://meet.example:443", PermissionKind::Camera),
            Some(true)
        );
        assert_eq!(
            manager.remembered("https://meet.example:443", PermissionKind::Microphone),
            Some(true)
        );
        assert_eq!(
            manager.remembered("https://meet.example:443", PermissionKind::Media),
            Some(true)
        );
    }

    #[test]
    fn ephemeral_manager_remembers_in_memory_only() {
        let manager = PermissionManager::ephemeral();

        assert_eq!(
            manager.remembered("example.com", PermissionKind::Media),
            None
        );

        manager.remember("example.com", PermissionKind::Media, true);

        assert_eq!(
            manager.remembered("example.com", PermissionKind::Media),
            Some(true)
        );
        // A combined grant is stored as two independent device decisions.
        assert_eq!(manager.decision_count(), 2);

        manager.clear_all();

        assert_eq!(manager.decision_count(), 0);
    }

    #[test]
    fn ephemeral_clones_share_one_window_scope() {
        // The window hands clones of its manager to every page and to the
        // Settings view; all of them must see the same decisions.
        let manager = PermissionManager::ephemeral();
        let page_copy = manager.clone();

        manager.remember(
            "https://meet.example:443",
            PermissionKind::Notification,
            true,
        );

        assert_eq!(
            page_copy.remembered("https://meet.example:443", PermissionKind::Notification),
            Some(true)
        );

        page_copy.remember(
            "https://maps.example:443",
            PermissionKind::Geolocation,
            false,
        );

        assert_eq!(
            manager.remembered("https://maps.example:443", PermissionKind::Geolocation),
            Some(false)
        );
        assert_eq!(manager.decision_count(), 2);
        assert_eq!(page_copy.decision_count(), 2);
    }

    #[test]
    fn separate_windows_never_share_ephemeral_decisions() {
        // A private window gets its own manager: the normal window's grants
        // must not leak in, and private decisions must not leak back out.
        let normal = PermissionManager::ephemeral();
        let private = PermissionManager::ephemeral();
        let origin = "https://meet.example:443";

        normal.remember(origin, PermissionKind::Camera, true);

        assert_eq!(private.remembered(origin, PermissionKind::Camera), None);

        private.remember(origin, PermissionKind::Microphone, true);
        private.remember(
            "https://private.example:443",
            PermissionKind::Notification,
            true,
        );

        assert_eq!(normal.remembered(origin, PermissionKind::Microphone), None);
        assert_eq!(
            normal.remembered("https://private.example:443", PermissionKind::Notification),
            None
        );
        assert_eq!(normal.decision_count(), 1);
        assert_eq!(private.decision_count(), 2);
    }

    #[test]
    fn clearing_a_private_window_leaves_normal_decisions_alone() {
        let normal = PermissionManager::ephemeral();
        let private = PermissionManager::ephemeral();
        let origin = "https://meet.example:443";

        normal.remember(origin, PermissionKind::Camera, true);
        private.remember(origin, PermissionKind::Camera, true);

        private.clear_all();

        assert_eq!(private.decision_count(), 0);
        assert_eq!(
            normal.remembered(origin, PermissionKind::Camera),
            Some(true)
        );

        // Closing the private window drops its manager and its grants.
        drop(private);

        assert_eq!(
            normal.remembered(origin, PermissionKind::Camera),
            Some(true)
        );
    }

    #[test]
    fn ephemeral_managers_have_no_storage_path() {
        assert!(PermissionManager::ephemeral().path.is_none());
        assert!(PermissionManager::ephemeral().clone().path.is_none());
    }

    #[test]
    fn private_decisions_never_reach_the_profile_file() {
        let path = std::env::temp_dir().join(format!(
            "axys-permissions-isolation-{}.toml",
            std::process::id()
        ));

        let _ = fs::remove_file(&path);

        let profile = PermissionManager {
            store: Rc::new(RefCell::new(PermissionStore::default())),
            path: Some(path.clone()),
        };
        let private = PermissionManager::ephemeral();

        profile.remember(
            "https://normal.example:443",
            PermissionKind::Notification,
            true,
        );
        private.remember("https://private.example:443", PermissionKind::Camera, true);

        let saved: PermissionStore =
            toml::from_str(&fs::read_to_string(&path).expect("profile file was written"))
                .expect("profile file is valid TOML");

        assert!(saved.decisions.contains_key("https://normal.example:443"));
        assert!(
            !saved.decisions.contains_key("https://private.example:443"),
            "private decisions must never be persisted"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn corrupt_store_loads_empty() {
        let path = std::env::temp_dir().join("axys-permissions-corrupt-test.toml");

        let _ = fs::write(&path, "not = [valid toml");

        assert!(load_store_for_test(&path).is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn legacy_media_decision_migrates_to_devices() {
        let mut store = PermissionStore::default();

        store
            .decisions
            .entry("https://meet.example:443".to_string())
            .or_default()
            .insert("media".to_string(), true);

        assert!(PermissionManager::migrate_legacy_media(&mut store));

        let kinds = store.decisions.get("https://meet.example:443").unwrap();

        assert_eq!(kinds.get("media"), None);
        assert_eq!(kinds.get("camera"), Some(&true));
        assert_eq!(kinds.get("microphone"), Some(&true));
    }

    #[test]
    fn classifies_capture_drm_and_pointer_lock() {
        assert_eq!(
            PermissionKind::classify("WebKitScreenCapturePermissionRequest"),
            PermissionKind::ScreenShare
        );
        assert_eq!(
            PermissionKind::classify("WebKitDisplayCapturePermissionRequest"),
            PermissionKind::ScreenShare
        );
        assert_eq!(
            PermissionKind::classify("WebKitMediaKeySystemPermissionRequest"),
            PermissionKind::Drm
        );
        assert_eq!(
            PermissionKind::classify("WebKitPointerLockPermissionRequest"),
            PermissionKind::PointerLock
        );
    }

    #[test]
    fn promptable_kinds_are_exactly_the_sensitive_ones() {
        for kind in [
            PermissionKind::Notification,
            PermissionKind::Geolocation,
            PermissionKind::Camera,
            PermissionKind::Microphone,
            PermissionKind::Media,
            PermissionKind::ScreenShare,
            PermissionKind::Drm,
            PermissionKind::Clipboard,
        ] {
            assert!(kind.needs_prompt(), "{kind:?}");
        }

        assert!(!PermissionKind::PointerLock.needs_prompt());
        assert!(!PermissionKind::Other.needs_prompt());
    }

    #[test]
    fn every_kind_has_a_unique_label_key() {
        let kinds = [
            PermissionKind::Notification,
            PermissionKind::Geolocation,
            PermissionKind::Camera,
            PermissionKind::Microphone,
            PermissionKind::Media,
            PermissionKind::ScreenShare,
            PermissionKind::Drm,
            PermissionKind::PointerLock,
            PermissionKind::Clipboard,
            PermissionKind::Other,
        ];

        let mut keys: Vec<&str> = kinds.iter().map(|kind| kind.label_key()).collect();

        keys.sort_unstable();
        keys.dedup();

        assert_eq!(keys.len(), kinds.len(), "label keys must be unique");
        assert!(kinds.iter().all(|kind| !kind.label_key().is_empty()));
    }

    #[test]
    fn generic_origin_is_never_recorded_or_consulted() {
        let manager = PermissionManager::ephemeral();

        manager.remember(GENERIC_ORIGIN, PermissionKind::Notification, true);

        assert_eq!(manager.decision_count(), 0);
        assert_eq!(
            manager.remembered(GENERIC_ORIGIN, PermissionKind::Notification),
            None
        );
    }

    #[test]
    fn combined_media_needs_camera_and_microphone_to_agree() {
        let origin = "https://meet.example:8443";

        let manager = PermissionManager::ephemeral();

        manager.remember(origin, PermissionKind::Camera, true);
        manager.remember(origin, PermissionKind::Microphone, false);

        assert_eq!(manager.remembered(origin, PermissionKind::Media), None);
        assert_eq!(
            manager.remembered(origin, PermissionKind::Camera),
            Some(true)
        );
        assert_eq!(
            manager.remembered(origin, PermissionKind::Microphone),
            Some(false)
        );
    }

    #[test]
    fn clipboard_setting_never_skips_the_prompt_for_unverifiable_origins() {
        assert!(clipboard_read_without_prompt(
            "https://example.com:443",
            true
        ));
        assert!(!clipboard_read_without_prompt(
            "https://example.com:443",
            false
        ));
        assert!(!clipboard_read_without_prompt(GENERIC_ORIGIN, true));
        assert!(!clipboard_read_without_prompt(GENERIC_ORIGIN, false));
    }

    #[test]
    fn origins_canonicalize_the_host_and_the_default_port() {
        // DNS is case-insensitive and a trailing root dot is the same name.
        assert_eq!(
            origin_for_uri(Some("https://Example.COM./Path?q=1#x")),
            "https://example.com:443"
        );
        assert_eq!(
            origin_for_uri(Some("http://EXAMPLE.com")),
            "http://example.com:80"
        );
        // Explicit ports are part of the identity.
        assert_eq!(
            origin_for_uri(Some("https://example.com:8443/x")),
            "https://example.com:8443"
        );
        // The scheme's case must not create a second identity.
        assert_eq!(
            origin_for_uri(Some("HTTPS://example.com")),
            origin_for_uri(Some("https://example.com"))
        );
    }

    #[test]
    fn an_explicit_port_zero_is_its_own_origin() {
        // GLib reports `-1` for "no port"; an explicit `:0` is a real port and
        // must not be folded into the scheme default.
        assert_eq!(
            origin_for_uri(Some("http://example.com:0/")),
            "http://example.com:0"
        );
        assert_eq!(
            origin_for_uri(Some("https://example.com:0/")),
            "https://example.com:0"
        );

        assert_ne!(
            origin_for_uri(Some("http://example.com:0")),
            origin_for_uri(Some("http://example.com")),
            ":0 must not share a grant with the default HTTP port"
        );
        assert_ne!(
            origin_for_uri(Some("https://example.com:0")),
            origin_for_uri(Some("https://example.com")),
            ":0 must not share a grant with the default HTTPS port"
        );

        // The omitted port still means the scheme default.
        assert_eq!(
            origin_for_uri(Some("https://example.com/")),
            "https://example.com:443"
        );
    }

    #[test]
    fn origins_keep_scheme_port_and_host_apart() {
        let https = origin_for_uri(Some("https://example.com"));
        let http = origin_for_uri(Some("http://example.com"));
        let other_port = origin_for_uri(Some("https://example.com:8443"));

        assert_ne!(https, http, "a TLS grant must not cover plain HTTP");
        assert_ne!(https, other_port, "another port is another origin");
        assert_ne!(http, other_port);
        assert_ne!(
            origin_for_uri(Some("https://example.com.evil.test")),
            https,
            "a look-alike host is a different site"
        );
    }

    #[test]
    fn ipv6_origins_keep_exactly_one_pair_of_brackets() {
        assert_eq!(
            origin_for_uri(Some("https://[::1]:8080/x")),
            "https://[::1]:8080"
        );
        assert_eq!(origin_for_uri(Some("https://[::1]/")), "https://[::1]:443");
    }

    #[test]
    fn hostless_and_non_web_pages_get_no_remembered_identity() {
        for uri in [
            None,
            Some(""),
            Some("   "),
            Some("about:blank"),
            Some("data:text/html,<p>hi</p>"),
            Some("blob:https://example.com/id"),
            Some("file:///home/user/page.html"),
            // A file URL's "host" is a machine name, not a web origin.
            Some("file://fileserver/share/page.html"),
        ] {
            assert_eq!(
                origin_for_uri(uri),
                GENERIC_ORIGIN,
                "{uri:?} must not get a stored identity"
            );
        }
    }

    #[test]
    fn a_file_page_never_reuses_or_stores_a_web_grant() {
        let manager = PermissionManager::ephemeral();
        let web = origin_for_uri(Some("https://example.com/"));

        manager.remember(&web, PermissionKind::Camera, true);

        let file = origin_for_uri(Some("file://fileserver/share/page.html"));

        assert_eq!(file, GENERIC_ORIGIN);
        assert_eq!(manager.remembered(&file, PermissionKind::Camera), None);

        manager.remember(&file, PermissionKind::Camera, true);

        assert_eq!(
            manager.decision_count(),
            1,
            "the file origin must not be stored"
        );
    }
}
