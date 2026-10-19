//! Per-origin website permission decisions.
//!
//! Replaces blanket auto-allow/auto-deny: the first time an origin asks for
//! something sensitive (notifications, camera/microphone, geolocation,
//! screen sharing, DRM), the browser prompts and can remember the answer.
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
}
