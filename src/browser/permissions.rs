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
    /// Camera and/or microphone (single WebKit user-media request).
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
        } else if lower.contains("display")
            || lower.contains("screen")
            || lower.contains("capture")
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

    /// Whether this kind ever prompts (as opposed to an automatic verdict).
    pub fn needs_prompt(self) -> bool {
        matches!(
            self,
            Self::Notification
                | Self::Geolocation
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

        let store = fs::read_to_string(&path)
            .ok()
            .and_then(|contents| toml::from_str::<PermissionStore>(&contents).ok())
            .unwrap_or_default();

        Self {
            store: Rc::new(RefCell::new(store)),
            path: Some(path),
        }
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
    pub fn remembered(&self, origin: &str, kind: PermissionKind) -> Option<bool> {
        if origin == GENERIC_ORIGIN {
            return None;
        }

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
    pub fn remember(&self, origin: &str, kind: PermissionKind, allowed: bool) {
        if origin == GENERIC_ORIGIN {
            return;
        }

        self.store
            .borrow_mut()
            .decisions
            .entry(origin.to_string())
            .or_default()
            .insert(kind_key(kind), allowed);

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
    fn ephemeral_manager_remembers_in_memory_only() {
        let manager = PermissionManager::ephemeral();

        assert_eq!(manager.remembered("example.com", PermissionKind::Media), None);

        manager.remember("example.com", PermissionKind::Media, true);

        assert_eq!(
            manager.remembered("example.com", PermissionKind::Media),
            Some(true)
        );
        assert_eq!(manager.decision_count(), 1);

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
}
