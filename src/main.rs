//! axysBrowser: a lightweight, privacy-focused web browser for Linux.
//!
//! Built with Rust + GTK4 + WebKitGTK 6. The crate is a single binary; the
//! interesting structure for contributors is:
//!
//! - [`browser`]: engine/session setup, address resolution + search engines,
//!   tabs model, bookmarks, history and downloads managers.
//! - [`ui`]: windows, toolbar, tab bar, bookmark bar, find bar and menus.
//!   Each window owns its tabs; [`ui::window`] wires everything together.
//! - [`internal`]: `axys://` pages (new tab, settings, history, …) and their
//!   router. They are plain GTK widgets, no web content.
//! - [`app_state`]: persisted user settings + shared managers (history,
//!   downloads, bookmarks) and the application CSS provider.
//! - [`paths`]: XDG data locations. [`theme`]: light/dark stylesheets.
//!   [`i18n`]: English/Spanish locale helpers (`locales/`).
//!
//! Privacy model: the normal window uses a persistent `NetworkSession`
//! (SQLite cookie jar, disk cache); every privacy window gets a fresh
//! ephemeral session, and history/downloads/bookmarks writes are gated on
//! `!private_mode` (see [`ui::window`]).

rust_i18n::i18n!("locales", fallback = "en");

mod app;
mod app_state;
mod browser;
mod i18n;
mod internal;
mod paths;
mod theme;
mod ui;

fn main() {
    paths::secure_dirs();

    app::run();
}
