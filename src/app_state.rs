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

        let network_session =
            BrowserEngine::create_network_session(settings.tracking_prevention);

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
            gtk_settings.set_property("gtk-application-prefer-dark-theme", dark);
        }

        self.css_provider.load_from_data(&theme::stylesheet(dark));
    }

    pub fn subscribe_language(&self, callback: &Rc<dyn Fn()>) {
        self.language_listeners
            .borrow_mut()
            .push(Rc::downgrade(callback));
    }

    pub fn subscribe_settings(&self, callback: &Rc<dyn Fn()>) {
        self.settings_listeners
            .borrow_mut()
            .push(Rc::downgrade(callback));
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

    /// Hardware acceleration for new tabs (existing ones keep theirs).
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
    /// next reload), and bookmarks are never touched.
    pub fn clear_browsing_data(&self) {
        self.history.clear();

        if let Some(cookie_manager) = self.network_session.cookie_manager() {
            cookie_manager.clone().all_cookies(
                None::<&webkit6::gio::Cancellable>,
                move |result| match result {
                    Ok(cookies) => {
                        for cookie in cookies {
                            cookie_manager.delete_cookie(
                                &cookie,
                                None::<&webkit6::gio::Cancellable>,
                                |result| {
                                    if let Err(error) = result {
                                        eprintln!("Failed to delete cookie: {error}");
                                    }
                                },
                            );
                        }
                    }
                    Err(error) => {
                        eprintln!("Failed to list cookies: {error}");
                    }
                },
            );
        }

        if let Some(data_manager) = self.network_session.website_data_manager()
            && let Some(favicons) = data_manager.favicon_database()
        {
            favicons.clear();
        }

        // Everything else the sites persisted: IndexedDB, local storage,
        // DOM/ServiceWorker caches, … (cookies are deleted above).
        if let Some(data_manager) = self.network_session.website_data_manager() {
            crate::browser::engine::BrowserEngine::clear_website_data(
                &data_manager,
                webkit6::WebsiteDataTypes::ALL,
            );
        }

        let cache = crate::browser::engine::web_cache_dir();

        if cache.exists() {
            if let Err(error) = fs::remove_dir_all(&cache) {
                eprintln!("Failed to clear disk cache: {error}");
            } else if let Err(error) = fs::create_dir_all(&cache) {
                eprintln!("Failed to recreate cache directory: {error}");
            }
        }
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
