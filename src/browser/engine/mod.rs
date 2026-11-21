//! The engine seam: the only module that selects a browser engine.
//!
//! Everything outside `browser::engine` reaches the engine through
//! `BrowserEngine` and through the session and view types re-exported here,
//! and adding a platform is a `cfg` plus one backend file instead of a change
//! spread across the interface.
//!
//! It is a facade, not yet an abstraction: the re-exported types are the
//! backend's own, so a handful of interface files still name WebKit types
//! (`ui/window.rs`, `ui/findbar.rs`, `browser/downloads.rs`, `ui/auth.rs`, the
//! session in `app_state.rs`). Replacing those with the seam's own types is
//! part of phase 4, when the Servo backend shows what the shared shape has to
//! be; a trait guessed against one implementation would only be rewritten.
//!
//! A backend is one module per platform, selected at compile time:
//!
//! * [`webkit`] on Linux, on WebKitGTK (today's engine, unchanged), and
//! * `servo` on Windows, on the Servo crate embedded as a library.
//!
//! Policy that is not an engine detail lives in `shared`, so the external-scheme
//! decision, the confirmation rule and the load-failure page cannot drift
//! between the two backends.
//!
//! What a backend must provide, because the rest of the tree calls it through
//! these names:
//!
//! * `BrowserEngine` with `create_network_session`, `create_ephemeral_session`,
//!   `configure`, `wire_view`, `is_external_scheme`, `open_external`, `load`,
//!   `reload`, `show_timeout_page`, `take_error_page`, `mark_source_view`,
//!   `take_source_view` and `clear_website_data`;
//! * the view and session types the interface stores and renders, plus the
//!   `AuthenticateHandler` it hands to `wire_view`;
//! * `apply_cookie_policy` and `web_cache_dir`;
//! * the engine-neutral types in [`types`], of which [`ViewConfig`] is the
//!   first to be shared (it replaces the two configuration booleans).
//!
//! Platform facts that are not engine facts stay in the backend that owns
//! them: the `/proc` and cgroup memory policy is Linux's, so it lives in
//! [`webkit`] rather than here.

mod shared;
mod types;

#[cfg(target_os = "linux")]
mod webkit;

#[cfg(target_os = "windows")]
mod servo;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
compile_error!(
    "axysBrowser embeds WebKitGTK on Linux and Servo on Windows; this target has no engine backend"
);

pub use types::{
    AuthPrompt, Credentials, FindOutcome, FindRequest, LoadState, MenuItem, ViewConfig,
};

#[cfg(target_os = "linux")]
pub use webkit::{AuthenticateHandler, BrowserEngine, web_cache_dir};

#[cfg(target_os = "linux")]
pub use webkit::{Download, Session, View, WeakView};

#[cfg(target_os = "windows")]
pub use servo::{AuthenticateHandler, BrowserEngine, web_cache_dir};

#[cfg(target_os = "windows")]
pub use servo::{Download, Session, View, WeakView};
