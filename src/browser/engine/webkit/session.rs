//! The seam's session, backed by WebKitGTK.
//!
//! A session owns cookies, cache and site data, and it is what creates pages,
//! so the interface never builds an engine session or view itself. `app_state`
//! holds the profile's persistent session; a private window creates its own
//! ephemeral one.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use gtk::gio;
use webkit6::{CookiePersistentStorage, NetworkSession, WebView, WebsiteDataTypes};

use super::BrowserEngine;
use super::download::Download;
use super::view::View;
use crate::browser::engine::types::ViewConfig;

/// A callback the engine may invoke at most once (its clear operations report
/// asynchronously, and some report never when the manager is missing).
type DoneSlot = Rc<std::cell::RefCell<Option<Box<dyn FnOnce(bool)>>>>;

/// A persistent or ephemeral browsing session.
#[derive(Clone)]
pub struct Session {
    inner: NetworkSession,
}

impl Session {
    /// The profile's session: persistent cookies, on-disk cache.
    pub fn persistent(itp_enabled: bool) -> Self {
        Self::wrap(BrowserEngine::create_network_session(itp_enabled))
    }

    /// A private window's session: nothing touches the profile.
    pub fn ephemeral(itp_enabled: bool) -> Self {
        let session = Self::wrap(BrowserEngine::create_ephemeral_session());

        session.set_itp(itp_enabled);

        session
    }

    fn wrap(inner: NetworkSession) -> Self {
        Self { inner }
    }

    /// Where cookies live on disk. WebKit does not persist them by itself.
    pub fn persist_cookies(&self, path: &Path) {
        let Some(manager) = self.inner.cookie_manager() else {
            return;
        };

        let path = path.to_string_lossy().into_owned();

        manager.set_persistent_storage(&path, CookiePersistentStorage::Sqlite);
    }

    pub fn apply_cookie_policy(&self, policy: &str) {
        super::apply_cookie_policy(&self.inner, policy);
    }

    pub fn set_itp(&self, enabled: bool) {
        self.inner.set_itp_enabled(enabled);
    }

    /// Creates a page in this session, configured from the user's settings.
    pub fn view(&self, config: &ViewConfig) -> View {
        let web_view = WebView::builder().network_session(&self.inner).build();

        BrowserEngine::configure(&web_view, config);

        View::from_webkit(web_view)
    }

    /// Deletes every cookie this session holds. Reports whether all of them
    /// were removed: a missing cookie manager counts as a failure, because
    /// then nothing was really wiped.
    pub fn clear_cookies(&self, done: impl FnOnce(bool) + 'static) {
        let Some(manager) = self.inner.cookie_manager() else {
            done(false);

            return;
        };

        let failures = Rc::new(Cell::new(false));
        let pending = Rc::new(Cell::new(1u32));
        let manager_for_loop = manager.clone();
        let finish: Rc<dyn Fn()> = {
            let failures = failures.clone();
            let pending = pending.clone();
            let done: DoneSlot = Rc::new(std::cell::RefCell::new(Some(
                Box::new(done) as Box<dyn FnOnce(bool)>
            )));

            Rc::new(move || {
                let left = pending.get() - 1;

                pending.set(left);

                if left == 0
                    && let Some(done) = done.borrow_mut().take()
                {
                    done(!failures.get());
                }
            })
        };

        {
            let failures = failures.clone();
            let pending = pending.clone();
            let finish = finish.clone();

            manager.all_cookies(gio::Cancellable::NONE, move |result| {
                match result {
                    Ok(cookies) => {
                        for cookie in cookies {
                            pending.set(pending.get() + 1);

                            let failures = failures.clone();
                            let finish = finish.clone();

                            manager_for_loop.delete_cookie(
                                &cookie,
                                gio::Cancellable::NONE,
                                move |result| {
                                    if result.is_err() {
                                        failures.set(true);
                                    }

                                    finish();
                                },
                            );
                        }
                    }
                    Err(error) => {
                        eprintln!("Failed to list cookies: {error}");
                        failures.set(true);
                    }
                }

                finish();
            });
        }
    }

    /// Wipes everything sites persisted in this session (IndexedDB, local
    /// storage, DOM/ServiceWorker caches, the HTTP cache, favicons). The
    /// on-disk cache directory is the caller's job, after this reports.
    pub fn clear_site_data(&self, done: impl FnOnce(bool) + 'static) {
        let Some(manager) = self.inner.website_data_manager() else {
            done(false);

            return;
        };

        if let Some(favicons) = manager.favicon_database() {
            favicons.clear();
        }

        let done: DoneSlot = Rc::new(std::cell::RefCell::new(Some(Box::new(done))));

        BrowserEngine::clear_website_data(
            &manager,
            WebsiteDataTypes::ALL,
            Some(Rc::new(move |ok| {
                if let Some(done) = done.borrow_mut().take() {
                    done(ok);
                }
            })),
        );
    }

    /// Every transfer started in this session, whether by a link, "Save image
    /// as" or a page script.
    pub fn on_download_started(&self, f: impl Fn(Download) + 'static) {
        self.inner.connect_download_started(move |_, download| {
            f(Download::from_webkit(download.clone()));
        });
    }
}
