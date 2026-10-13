use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::env;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gtk::glib;
use webkit6::gdk;
use webkit6::gdk::prelude::TextureExt;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,

    #[serde(default)]
    pub favicon_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BookmarkStore {
    bookmarks: Vec<Bookmark>,
}

type Listener = Weak<dyn Fn()>;
type Listeners = Rc<RefCell<Vec<Listener>>>;

#[derive(Clone)]
pub struct BookmarkManager {
    bookmarks: Rc<RefCell<Vec<Bookmark>>>,
    path: PathBuf,
    listeners: Listeners,
}

impl BookmarkManager {
    pub fn load() -> Self {
        let path = Self::storage_path();

        let bookmarks = fs::read_to_string(&path)
            .ok()
            .and_then(|contents| {
                toml::from_str::<BookmarkStore>(&contents)
                    .ok()
                    .map(|store| store.bookmarks)
            })
            .unwrap_or_default();

        Self {
            bookmarks: Rc::new(RefCell::new(bookmarks)),
            path,
            listeners: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn entries(&self) -> Vec<Bookmark> {
        self.bookmarks.borrow().clone()
    }

    pub fn get(&self, url: &str) -> Option<Bookmark> {
        self.bookmarks
            .borrow()
            .iter()
            .find(|bookmark| bookmark.url == url)
            .cloned()
    }

    pub fn contains(&self, url: &str) -> bool {
        self.bookmarks
            .borrow()
            .iter()
            .any(|bookmark| bookmark.url == url)
    }

    pub fn add_with_favicon(
        &self,
        title: impl Into<String>,
        url: impl Into<String>,
        favicon: Option<&gdk::Texture>,
    ) -> bool {
        let url = url.into();

        if !Self::is_bookmarkable_url(&url) {
            return false;
        }

        let title = sanitize_title(title.into(), &url);

        let favicon_path = favicon.and_then(|texture| {
            Self::save_favicon(&url, texture)
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        });

        {
            let mut bookmarks = self.bookmarks.borrow_mut();

            if bookmarks.iter().any(|bookmark| bookmark.url == url) {
                return false;
            }

            bookmarks.push(Bookmark {
                title,
                url,
                favicon_path,
            });
        }

        self.persist_and_notify();

        true
    }

    pub fn update_with_favicon(
        &self,
        old_url: &str,
        title: impl Into<String>,
        new_url: impl Into<String>,
        favicon: Option<&gdk::Texture>,
    ) -> bool {
        let new_url = new_url.into();

        if !Self::is_bookmarkable_url(&new_url) {
            return false;
        }

        let title = sanitize_title(title.into(), &new_url);

        let new_favicon = favicon.and_then(|texture| {
            Self::save_favicon(&new_url, texture)
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        });

        {
            let mut bookmarks = self.bookmarks.borrow_mut();

            if old_url != new_url && bookmarks.iter().any(|bookmark| bookmark.url == new_url) {
                return false;
            }

            let Some(bookmark) = bookmarks
                .iter_mut()
                .find(|bookmark| bookmark.url == old_url)
            else {
                return false;
            };

            bookmark.title = title;
            bookmark.url = new_url;

            if new_favicon.is_some() {
                bookmark.favicon_path = new_favicon;
            }
        }

        self.persist_and_notify();

        true
    }

    pub fn remove(&self, url: &str) -> bool {
        let removed_favicon = {
            let mut bookmarks = self.bookmarks.borrow_mut();

            let Some(bookmark) = bookmarks.iter().find(|bookmark| bookmark.url == url) else {
                return false;
            };

            let favicon = bookmark.favicon_path.clone();

            bookmarks.retain(|bookmark| bookmark.url != url);

            favicon
        };

        if let Some(path) = removed_favicon.as_deref() {
            let _ = fs::remove_file(path);
        }

        self.persist_and_notify();

        true
    }

    pub fn subscribe(&self, callback: &Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(Rc::downgrade(callback));
    }

    fn persist_and_notify(&self) {
        if let Err(error) = self.save() {
            eprintln!("Failed to save bookmarks: {error}");
        }

        self.notify();
    }

    fn notify(&self) {
        let callbacks = {
            let mut listeners = self.listeners.borrow_mut();

            listeners.retain(|listener| listener.strong_count() > 0);

            listeners.clone()
        };

        for listener in callbacks {
            if let Some(callback) = listener.upgrade() {
                callback();
            }
        }
    }

    fn is_bookmarkable_url(url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }

    fn storage_path() -> PathBuf {
        config_directory().join("bookmarks.toml")
    }

    fn favicon_directory() -> PathBuf {
        cache_directory().join("favicons")
    }

    fn favicon_path_for(url: &str) -> PathBuf {
        let mut hasher = DefaultHasher::new();

        url.hash(&mut hasher);

        let hash = hasher.finish();

        Self::favicon_directory().join(format!("{hash:016x}.png"))
    }

    fn save_favicon(url: &str, texture: &gdk::Texture) -> Result<PathBuf, glib::BoolError> {
        let directory = Self::favicon_directory();

        fs::create_dir_all(&directory)
            .map_err(|error| glib::bool_error!("Failed to create favicon directory: {error}"))?;

        let path = Self::favicon_path_for(url);

        texture.save_to_png(&path)?;

        Ok(path)
    }

    fn save(&self) -> io::Result<()> {
        let bookmarks = self.bookmarks.borrow();

        let store = BookmarkStore {
            bookmarks: bookmarks.clone(),
        };

        let contents = toml::to_string_pretty(&store).map_err(io::Error::other)?;

        atomic_write(&self.path, &contents)
    }
}

fn sanitize_title(title: String, url: &str) -> String {
    let title = title.trim();

    if title.is_empty() {
        url.to_string()
    } else {
        title.to_string()
    }
}

fn config_directory() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("axysbrowser");
    }

    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".config").join("axysbrowser");
    }

    PathBuf::from("axysbrowser")
}

fn cache_directory() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("axysbrowser");
    }

    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".cache").join("axysbrowser");
    }

    PathBuf::from("axysbrowser-cache")
}

fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bookmarks path has no parent",
        ));
    };

    fs::create_dir_all(parent)?;

    let temporary = path.with_extension("tmp");

    fs::write(&temporary, contents)?;
    fs::rename(temporary, path)
}
