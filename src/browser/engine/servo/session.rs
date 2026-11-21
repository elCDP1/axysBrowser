//! The seam's session, backed by Servo.
//!
//! Servo 0.7 is one engine per process with one profile: the cookie jar, the
//! site data and the (in-memory) HTTP cache are global, there is no
//! per-session partition and no cookie accept policy in the embedder API.
//!
//! This type therefore exists so the interface's session vocabulary keeps
//! working — a profile session for normal windows, an ephemeral one for
//! private windows — and it says plainly which parts the engine cannot honour
//! instead of pretending. `docs/porting/windows-servo.md` carries the list;
//! the two entries that matter for privacy are that a private window is **not**
//! isolated, and that the third-party-cookie policy is not enforced.
//!
//! There is no download hook either: Servo 0.7 cannot start a transfer, so
//! [`server::download::Download`](super::download) has no values and nothing
//! could ever call one.

use std::path::Path;

use servo::StorageType;

use super::BrowserEngine;
use super::view::View;
use crate::browser::engine::types::ViewConfig;

/// A browsing session.
#[derive(Clone, Default)]
pub struct Session;

impl Session {
    /// The profile's session. `itp_enabled` is accepted and unused: Servo 0.7
    /// has no tracking-prevention switch.
    pub fn persistent(itp_enabled: bool) -> Self {
        BrowserEngine::create_network_session(itp_enabled)
    }

    /// A private window's session.
    ///
    /// Servo has a single profile, so this is the same session as the profile's
    /// and nothing is isolated. It is created through its own constructor so
    /// the gap is visible here, in one place, instead of hidden behind a
    /// shared one.
    pub fn ephemeral(_itp_enabled: bool) -> Self {
        BrowserEngine::create_ephemeral_session()
    }

    /// Where cookies live on disk. Servo keeps its cookie jar inside its own
    /// profile directory, so there is no per-session file to point at.
    pub fn persist_cookies(&self, _path: &Path) {}

    /// Normalizes and records a cookie-policy setting. Servo 0.7 exposes no
    /// cookie accept policy, so this cannot apply it; the Settings page still
    /// reads back what the user chose.
    pub fn apply_cookie_policy(&self, _policy: &str) {}

    pub fn set_itp(&self, _enabled: bool) {}

    /// Creates a page in this session, configured from the user's settings.
    pub fn view(&self, config: &ViewConfig) -> View {
        let view = View::new();

        BrowserEngine::configure(&view, config);

        view
    }

    /// Deletes every cookie this engine holds.
    ///
    /// The engine reports completion through the same callback, so a missing
    /// manager cannot silently look like success.
    pub fn clear_cookies(&self, done: impl FnOnce(bool) + 'static) {
        let servo = super::servo();

        servo
            .site_data_manager()
            .clear_cookies(Some(Box::new(move || done(true))));
    }

    /// Wipes cookie, local and session storage for every site the engine knows
    /// about, then the HTTP cache.
    ///
    /// The sites are listed first and passed by name: an empty slice would be
    /// ambiguous, and this way the call means exactly "every site that has
    /// data right now".
    pub fn clear_site_data(&self, done: impl FnOnce(bool) + 'static) {
        let servo = super::servo();
        let types = StorageType::Cookies | StorageType::Local | StorageType::Session;

        let sites: Vec<String> = servo
            .site_data_manager()
            .site_data(types)
            .into_iter()
            .map(|data| data.name())
            .collect();
        let names: Vec<&str> = sites.iter().map(String::as_str).collect();

        servo.site_data_manager().clear_site_data(&names, types);
        servo.network_manager().clear_cache();

        done(true);
    }
}
