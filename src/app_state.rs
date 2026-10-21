use crate::browser::address::{SearchConfig, load_config};
use crate::browser::bookmarks::BookmarkManager;
use crate::browser::downloads::DownloadManager;
use crate::browser::engine::BrowserEngine;
use crate::browser::history::HistoryManager;
use crate::browser::permissions::PermissionManager;
use crate::i18n;
use crate::theme;

use gtk::Application;
use gtk::CssProvider;
use gtk::Settings as GtkSettings;
use gtk::prelude::*;

use serde::{Deserialize, Serialize};

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use webkit6::{CookiePersistentStorage, NetworkSession};

/// Removes a directory's contents (the directory itself is recreated).
/// A missing directory counts as already clean.
fn wipe_dir(dir: &std::path::Path) -> bool {
    if dir.exists()
        && let Err(error) = fs::remove_dir_all(dir)
    {
        eprintln!("Failed to clear {}: {error}", dir.display());
        return false;
    }

    if let Err(error) = fs::create_dir_all(dir) {
        eprintln!("Failed to recreate {}: {error}", dir.display());
        return false;
    }

    true
}

type DoneCallback = Rc<dyn Fn(bool)>;

/// Aggregates several (possibly asynchronous) steps into one verdict:
/// the callback fires exactly once, when the last step finished, with
/// `true` only if no step failed.
#[derive(Clone)]
struct Completion {
    pending: Rc<Cell<usize>>,
    failed: Rc<Cell<bool>>,
    done: Rc<RefCell<Option<DoneCallback>>>,
}

impl Completion {
    fn new(done: Option<DoneCallback>) -> Self {
        Self {
            pending: Rc::new(Cell::new(0)),
            failed: Rc::new(Cell::new(false)),
            done: Rc::new(RefCell::new(done)),
        }
    }

    fn begin(&self) {
        self.pending.set(self.pending.get() + 1);
    }

    fn fail(&self) {
        self.failed.set(true);
    }

    fn finish(&self, ok: bool) {
        if !ok {
            self.failed.set(true);
        }

        let left = self.pending.get().saturating_sub(1);

        self.pending.set(left);

        if left == 0
            && let Some(callback) = self.done.borrow_mut().take()
        {
            callback(!self.failed.get());
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_search_engine() -> String {
    "brave".to_string()
}

fn default_cookie_policy() -> String {
    "no-third-party".to_string()
}

fn default_language() -> String {
    // Default to English; the user can switch to Spanish in Settings.
    // (Using the system locale here made fresh profiles start in Spanish
    // on es systems, which testers did not want as default.)
    "en".to_string()
}

type LanguageListener = Weak<dyn Fn()>;
type LanguageListeners = Rc<RefCell<Vec<LanguageListener>>>;
type SettingsListener = Weak<dyn Fn()>;
type SettingsListeners = Rc<RefCell<Vec<SettingsListener>>>;

/// Applies the app's light/dark choice to GTK.
///
/// GTK 4.20 replaced the `gtk-application-prefer-dark-theme` boolean with the
/// `gtk-interface-color-scheme` enum and warns whenever the old property is
/// used. The enum type in the `gtk4` bindings is behind its `v4_20` feature,
/// which this crate cannot enable (builds must keep working on GTK 4.10/4.14),
/// so the property is set by name. `g_object_set` takes enums as `int`, and
/// `GtkInterfaceColorScheme` keeps `DARK` at 2 across versions.
fn set_gtk_color_scheme(gtk_settings: &GtkSettings, dark: bool) {
    if gtk_settings
        .find_property("gtk-interface-color-scheme")
        .is_some()
    {
        // GtkInterfaceColorScheme: DEFAULT = 1, DARK = 2, LIGHT = 3.
        let scheme: i32 = if dark { 2 } else { 1 };

        unsafe {
            gtk::glib::gobject_ffi::g_object_set(
                gtk_settings.as_ptr().cast(),
                c"gtk-interface-color-scheme".as_ptr(),
                scheme,
                std::ptr::null::<std::ffi::c_void>(),
            );
        }
    } else {
        gtk_settings.set_property("gtk-application-prefer-dark-theme", dark);
    }
}

/// Browsing-data wipe with completion: receives a one-shot callback that
/// fires with success once the asynchronous wipe finishes.
pub type ClearBrowsingData = Rc<dyn Fn(Rc<dyn Fn(bool)>)>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserSettings {
    #[serde(default = "default_true")]
    pub dark_mode: bool,

    #[serde(default = "default_search_engine")]
    pub search_engine: String,

    #[serde(default = "default_true")]
    pub developer_tools: bool,

    #[serde(default = "default_true")]
    pub hardware_acceleration: bool,

    #[serde(default = "default_true")]
    pub tracking_prevention: bool,

    #[serde(default = "default_cookie_policy")]
    pub cookie_policy: String,

    #[serde(default = "default_language")]
    pub language: String,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            search_engine: default_search_engine(),
            developer_tools: true,
            hardware_acceleration: true,
            tracking_prevention: true,
            cookie_policy: default_cookie_policy(),
            language: default_language(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub settings: Rc<RefCell<UserSettings>>,
    pub search: Rc<RefCell<SearchConfig>>,
    pub network_session: NetworkSession,
    pub css_provider: CssProvider,
    pub css_installed: Rc<Cell<bool>>,
    pub downloads: DownloadManager,
    pub history: HistoryManager,
    pub bookmarks: BookmarkManager,
    pub permissions: PermissionManager,
    language_listeners: LanguageListeners,
    settings_listeners: SettingsListeners,
}

impl AppState {
    pub fn load(application: &Application) -> Rc<Self> {
        let mut settings = Self::load_user_settings();

        settings.language = i18n::normalize_locale(&settings.language).to_string();

        i18n::set_locale(&settings.language);

        // A hand-edited settings file could contain anything — normalize.
        let cookie_policy = std::mem::take(&mut settings.cookie_policy);

        settings.cookie_policy = match cookie_policy.as_str() {
            "always" | "never" => cookie_policy,
            _ => default_cookie_policy(),
        };

        let mut search = load_config();

        if !search.engines.contains_key(&settings.search_engine) {
            settings.search_engine = search.default_engine.clone();
        }

        search.default_engine = settings.search_engine.clone();

        let network_session = BrowserEngine::create_network_session(settings.tracking_prevention);

        /*
         * WebKit does not persist cookies automatically.
         * Use an explicit SQLite cookie store for the
         * normal persistent browser session.
         */
        if let Some(cookie_manager) = network_session.cookie_manager() {
            let cookie_path = Self::cookie_path();

            if let Some(parent) = cookie_path.parent()
                && let Err(error) = fs::create_dir_all(parent)
            {
                eprintln!("Failed to create cookie directory: {error}");
            }

            let cookie_path = cookie_path.to_string_lossy().into_owned();

            cookie_manager.set_persistent_storage(&cookie_path, CookiePersistentStorage::Sqlite);
        }

        crate::browser::engine::apply_cookie_policy(&network_session, &settings.cookie_policy);

        let downloads = DownloadManager::new(application.clone());

        downloads.watch(&network_session);

        let history = HistoryManager::load();

        let bookmarks = BookmarkManager::load();

        let permissions = PermissionManager::load();

        let state = Rc::new(Self {
            settings: Rc::new(RefCell::new(settings)),
            search: Rc::new(RefCell::new(search)),
            network_session,
            css_provider: CssProvider::new(),
            css_installed: Rc::new(Cell::new(false)),
            downloads,
            history,
            bookmarks,
            permissions,
            language_listeners: Rc::new(RefCell::new(Vec::new())),
            settings_listeners: Rc::new(RefCell::new(Vec::new())),
        });

        state.apply_theme();

        state
    }

    fn config_path() -> PathBuf {
        crate::paths::config_dir().join("settings.toml")
    }

    fn cookie_path() -> PathBuf {
        crate::paths::config_dir().join("cookies.sqlite")
    }

    fn load_user_settings() -> UserSettings {
        let path = Self::config_path();

        let Ok(text) = fs::read_to_string(path) else {
            return UserSettings::default();
        };

        let mut settings: UserSettings = toml::from_str(&text).unwrap_or_default();

        settings.language = i18n::normalize_locale(&settings.language).to_string();

        settings
    }

    pub fn save(&self) {
        let path = Self::config_path();

        let settings = self.settings.borrow();

        if let Ok(text) = toml::to_string_pretty(&*settings)
            && let Err(error) = crate::paths::atomic_write(&path, &text)
        {
            eprintln!("Failed to save settings: {error}");
        }
    }

    pub fn apply_theme(&self) {
        let dark = self.settings.borrow().dark_mode;

        if let Some(gtk_settings) = GtkSettings::default() {
            set_gtk_color_scheme(&gtk_settings, dark);
        }

        self.css_provider.load_from_data(&theme::stylesheet(dark));
    }

    pub fn subscribe_language(&self, callback: &Rc<dyn Fn()>) {
        let mut listeners = self.language_listeners.borrow_mut();

        // Drop dead entries eagerly: windows are opened and closed over a
        // long session, and this list only got pruned while notifying, so a
        // window closed right after opening could linger until the next
        // language change.
        listeners.retain(|listener| listener.strong_count() > 0);
        listeners.push(Rc::downgrade(callback));
    }

    pub fn subscribe_settings(&self, callback: &Rc<dyn Fn()>) {
        let mut listeners = self.settings_listeners.borrow_mut();

        listeners.retain(|listener| listener.strong_count() > 0);
        listeners.push(Rc::downgrade(callback));
    }

    fn notify_settings_changed(&self) {
        let mut listeners = self.settings_listeners.borrow_mut();

        listeners.retain(|listener| listener.strong_count() > 0);

        let active_listeners = listeners.clone();

        drop(listeners);

        for listener in active_listeners {
            if let Some(callback) = listener.upgrade() {
                callback();
            }
        }
    }

    fn notify_language_changed(&self) {
        let mut listeners = self.language_listeners.borrow_mut();

        listeners.retain(|listener| listener.strong_count() > 0);

        let active_listeners = listeners.clone();

        drop(listeners);

        for listener in active_listeners {
            if let Some(callback) = listener.upgrade() {
                callback();
            }
        }
    }

    pub fn set_search_engine(&self, engine: &str) {
        if !self.search.borrow().engines.contains_key(engine) {
            return;
        }

        self.search.borrow_mut().default_engine = engine.to_string();

        self.settings.borrow_mut().search_engine = engine.to_string();

        self.save();

        // Live-update subscribers (e.g. the address placeholder).
        self.notify_settings_changed();
    }

    pub fn set_dark_mode(&self, dark: bool) {
        self.settings.borrow_mut().dark_mode = dark;

        self.apply_theme();
        self.save();
    }

    pub fn set_developer_tools(&self, enabled: bool) {
        self.settings.borrow_mut().developer_tools = enabled;

        self.save();

        self.notify_settings_changed();
    }

    /// Hardware acceleration for every open tab (applied live).
    pub fn set_hardware_acceleration(&self, enabled: bool) {
        self.settings.borrow_mut().hardware_acceleration = enabled;

        self.save();

        self.notify_settings_changed();
    }

    /// Forgets every remembered website permission decision.
    pub fn reset_permissions(&self) {
        self.permissions.clear_all();
    }

    pub fn set_tracking_prevention(&self, enabled: bool) {
        self.settings.borrow_mut().tracking_prevention = enabled;

        self.network_session.set_itp_enabled(enabled);

        self.save();
    }

    pub fn set_cookie_policy(&self, policy: &str) {
        let normalized = match policy {
            "always" | "never" => policy.to_string(),
            _ => "no-third-party".to_string(),
        };

        if self.settings.borrow().cookie_policy == normalized {
            return;
        }

        self.settings.borrow_mut().cookie_policy = normalized.clone();

        crate::browser::engine::apply_cookie_policy(&self.network_session, &normalized);

        self.save();
    }

    /// Real "clear browsing data": history, all cookies, favicon cache,
    /// persistent website data (IndexedDB, local storage, DOM caches, …)
    /// and the HTTP disk cache. Open tabs are left alone (sites log out on
    /// next reload), and bookmarks and downloaded files are never touched.
    /// `on_done` fires once every step finished, reporting success only
    /// when ALL of them succeeded — including when a required manager is
    /// missing, because then nothing was really wiped (pass `None` to skip
    /// the report).
    /// It acts on the given `session`/`history` so a private window clears
    /// only its own ephemeral data (and never the profile's on-disk cache).
    /// Normal windows pass the profile session/history and `true`.
    pub fn clear_browsing_data(
        &self,
        session: &NetworkSession,
        history: &HistoryManager,
        wipe_disk_cache: bool,
        on_done: Option<Rc<dyn Fn(bool)>>,
    ) {
        let completion = Completion::new(on_done);

        // Setup token: keeps the aggregate open until every step below has
        // been registered, so a step finishing synchronously cannot report
        // success early.
        completion.begin();

        if let Err(error) = history.clear() {
            eprintln!("Failed to clear browsing history: {error}");
            completion.fail();
        }

        if let Some(cookie_manager) = session.cookie_manager() {
            completion.begin();

            let completion = completion.clone();

            cookie_manager
                .clone()
                .all_cookies(
                    None::<&webkit6::gio::Cancellable>,
                    move |result| match result {
                        Ok(cookies) => {
                            for cookie in cookies {
                                completion.begin();

                                let completion = completion.clone();

                                cookie_manager.delete_cookie(
                                    &cookie,
                                    None::<&webkit6::gio::Cancellable>,
                                    move |result| {
                                        if let Err(error) = &result {
                                            eprintln!("Failed to delete cookie: {error}");
                                        }

                                        completion.finish(result.is_ok());
                                    },
                                );
                            }

                            completion.finish(true);
                        }
                        Err(error) => {
                            eprintln!("Failed to list cookies: {error}");
                            completion.finish(false);
                        }
                    },
                );
        } else {
            eprintln!("No cookie manager: cookies could not be cleared");
            completion.fail();
        }

        if let Some(data_manager) = session.website_data_manager() {
            if let Some(favicons) = data_manager.favicon_database() {
                favicons.clear();
            }

            // Everything else the sites persisted: IndexedDB, local storage,
            // DOM/ServiceWorker caches, … The HTTP disk cache directory is
            // wiped only after WebKit finished, so the two never race.
            completion.begin();

            let completion = completion.clone();

            crate::browser::engine::BrowserEngine::clear_website_data(
                &data_manager,
                webkit6::WebsiteDataTypes::ALL,
                Some(Rc::new(move |ok| {
                    let wiped =
                        !wipe_disk_cache || wipe_dir(&crate::browser::engine::web_cache_dir());

                    completion.finish(ok && wiped);
                })),
            );
        } else {
            eprintln!("No website data manager: website data could not be cleared");
            completion.fail();
        }

        // Release the setup token; reports now if everything already ended.
        completion.finish(true);

        // Safety net: a WebKit async step that never reports back must not
        // leave the settings page stuck on "Clearing…". `Completion` reports
        // exactly once, so a late real callback after this is harmless.
        let watchdog = completion.clone();

        gtk::glib::timeout_add_seconds_local_once(30, move || watchdog.finish(false));
    }

    pub fn set_language(&self, language: &str) {
        let language = i18n::normalize_locale(language);

        if self.settings.borrow().language == language {
            return;
        }

        i18n::set_locale(language);

        self.settings.borrow_mut().language = language.to_string();

        self.save();

        self.notify_language_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Log = Rc<RefCell<Vec<bool>>>;

    fn recorder() -> (Log, DoneCallback) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let sink = log.clone();

        (log, Rc::new(move |ok| sink.borrow_mut().push(ok)))
    }

    #[test]
    fn completion_reports_success_once_after_all_steps() {
        let (log, callback) = recorder();
        let completion = Completion::new(Some(callback));

        completion.begin(); // setup token
        completion.begin();
        completion.begin();

        completion.finish(true);
        completion.finish(true);
        assert!(log.borrow().is_empty(), "must wait for every step");

        completion.finish(true); // releases the setup token
        assert_eq!(*log.borrow(), vec![true]);

        completion.begin();
        completion.finish(true);
        assert_eq!(log.borrow().len(), 1, "callback is one-shot");
    }

    #[test]
    fn completion_reports_failure_if_any_step_fails() {
        let (log, callback) = recorder();
        let completion = Completion::new(Some(callback));

        completion.begin();
        completion.begin();
        completion.fail(); // e.g. a missing WebsiteDataManager
        completion.finish(true);
        completion.finish(true);

        assert_eq!(*log.borrow(), vec![false]);
    }

    #[test]
    fn completion_with_only_the_setup_token_reports_success() {
        let (log, callback) = recorder();
        let completion = Completion::new(Some(callback));

        completion.begin();
        completion.finish(true);

        assert_eq!(*log.borrow(), vec![true]);
    }

    #[test]
    fn wipe_dir_empties_and_recreates() {
        let dir = std::env::temp_dir().join("axys-wipe-dir-test");

        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/file"), "x").unwrap();

        assert!(wipe_dir(&dir));
        assert!(dir.is_dir());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);

        let _ = fs::remove_dir_all(&dir);

        assert!(wipe_dir(&dir), "missing dir counts as clean");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_settings_defaults_are_stable() {
        let settings = UserSettings::default();

        assert!(settings.dark_mode);
        assert_eq!(settings.search_engine, "brave");
        assert!(settings.developer_tools);
        assert!(settings.hardware_acceleration);
        assert!(settings.tracking_prevention);
        assert_eq!(settings.cookie_policy, "no-third-party");
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn partial_settings_file_fills_in_defaults() {
        let settings: UserSettings =
            toml::from_str("dark_mode = false\nlanguage = \"es-ES\"").unwrap();

        assert!(!settings.dark_mode);
        assert_eq!(settings.language, "es-ES");

        // Anything omitted must come from the defaults, not from `false`/"".
        assert!(settings.developer_tools);
        assert!(settings.hardware_acceleration);
        assert!(settings.tracking_prevention);
        assert_eq!(settings.search_engine, "brave");
        assert_eq!(settings.cookie_policy, "no-third-party");
    }

    #[test]
    fn empty_settings_file_is_all_defaults() {
        let settings: UserSettings = toml::from_str("").unwrap();
        let defaults = UserSettings::default();

        assert_eq!(settings.dark_mode, defaults.dark_mode);
        assert_eq!(settings.search_engine, defaults.search_engine);
        assert_eq!(settings.cookie_policy, defaults.cookie_policy);
        assert_eq!(settings.language, defaults.language);
    }
}
