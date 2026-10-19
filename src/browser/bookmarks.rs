use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::PathBuf;
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

    /// Canonicalises a URL just enough to compare bookmarks: drops a single
    /// trailing slash (`https://example.com` == `https://example.com/`) and
    /// lower-cases the scheme and authority (`HTTPS://Example.com/` ==
    /// `https://example.com/`). The path stays case-sensitive as required by
    /// RFC 3986.
    fn normalize_url(url: &str) -> String {
        let url = url.strip_suffix('/').unwrap_or(url);

        let Some(sep) = url.find("://") else {
            return url.to_ascii_lowercase();
        };

        let scheme = url[..sep].to_ascii_lowercase();

        let rest = &url[sep + 3..];

        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());

        let (authority, tail) = rest.split_at(authority_end);

        format!("{scheme}://{}{tail}", authority.to_ascii_lowercase())
    }

    /// Compares URLs after [`normalize_url`](Self::normalize_url).
    fn same_url(a: &str, b: &str) -> bool {
        Self::normalize_url(a) == Self::normalize_url(b)
    }

    pub fn get(&self, url: &str) -> Option<Bookmark> {
        self.bookmarks
            .borrow()
            .iter()
            .find(|bookmark| Self::same_url(&bookmark.url, url))
            .cloned()
    }

    pub fn contains(&self, url: &str) -> bool {
        self.bookmarks
            .borrow()
            .iter()
            .any(|bookmark| Self::same_url(&bookmark.url, url))
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

        // Deduplicate before writing a favicon so a rejected add never
        // leaves an orphaned PNG behind.
        if self.contains(&url) {
            return false;
        }

        let favicon_path = favicon.and_then(|texture| {
            Self::save_favicon(&url, texture)
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        });

        self.bookmarks.borrow_mut().push(Bookmark {
            title,
            url,
            favicon_path,
        });

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

        // Locate the bookmark first: a failed edit must not write a favicon.
        let index = {
            let bookmarks = self.bookmarks.borrow();

            match bookmarks
                .iter()
                .position(|bookmark| Self::same_url(&bookmark.url, old_url))
            {
                Some(index) => index,
                None => return false,
            }
        };

        // Reject when the target URL already belongs to another bookmark.
        if self.bookmarks.borrow().iter().enumerate().any(|(i, bookmark)| {
            i != index && Self::same_url(&bookmark.url, &new_url)
        }) {
            return false;
        }

        let new_favicon = favicon.and_then(|texture| {
            Self::save_favicon(&new_url, texture)
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        });

        let old_favicon = {
            let mut bookmarks = self.bookmarks.borrow_mut();

            let bookmark = &mut bookmarks[index];

            let old_favicon = bookmark.favicon_path.clone();

            bookmark.title = title;
            bookmark.url = new_url;

            if new_favicon.is_some() {
                bookmark.favicon_path = new_favicon;
            }

            old_favicon
        };

        // Drop the previous favicon file once nothing references it, so
        // editing a URL does not leak a PNG forever.
        if let Some(old_favicon) = old_favicon.as_deref()
            && !self.favicon_referenced(old_favicon)
        {
            let _ = fs::remove_file(old_favicon);
        }

        self.persist_and_notify();

        true
    }

    pub fn remove(&self, url: &str) -> bool {
        let removed_favicon = {
            let mut bookmarks = self.bookmarks.borrow_mut();

            let Some(bookmark) = bookmarks
                .iter()
                .find(|bookmark| Self::same_url(&bookmark.url, url))
            else {
                return false;
            };

            let favicon = bookmark.favicon_path.clone();

            bookmarks.retain(|bookmark| !Self::same_url(&bookmark.url, url));

            favicon
        };

        if let Some(path) = removed_favicon.as_deref()
            && !self.favicon_referenced(path)
        {
            let _ = fs::remove_file(path);
        }

        self.persist_and_notify();

        true
    }

    /// Whether any stored bookmark still points at this favicon file.
    fn favicon_referenced(&self, path: &str) -> bool {
        self.bookmarks
            .borrow()
            .iter()
            .any(|bookmark| bookmark.favicon_path.as_deref() == Some(path))
    }

    pub fn subscribe(&self, callback: &Rc<dyn Fn()>) {
        let mut listeners = self.listeners.borrow_mut();

        listeners.retain(|listener| listener.strong_count() > 0);
        listeners.push(Rc::downgrade(callback));
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
        crate::paths::config_dir().join("bookmarks.toml")
    }

    fn favicon_directory() -> PathBuf {
        crate::paths::cache_dir().join("favicons")
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

        crate::paths::atomic_write(&self.path, &contents)
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

#[cfg(test)]
mod tests {
    use super::BookmarkManager;

    #[test]
    fn same_url_ignores_one_trailing_slash() {
        assert!(BookmarkManager::same_url(
            "https://example.com",
            "https://example.com/"
        ));
    }

    #[test]
    fn same_url_does_not_strip_multiple_slashes() {
        assert!(!BookmarkManager::same_url(
            "https://example.com/",
            "https://example.com//"
        ));
    }

    #[test]
    fn same_url_normalizes_scheme_and_host_case() {
        assert!(BookmarkManager::same_url(
            "HTTPS://Example.COM/Path",
            "https://example.com/Path"
        ));
    }

    #[test]
    fn same_url_keeps_path_case_sensitive() {
        assert!(!BookmarkManager::same_url(
            "https://example.com/Path",
            "https://example.com/path"
        ));
    }

    #[test]
    fn same_url_distinguishes_different_urls() {
        assert!(!BookmarkManager::same_url("https://a.com", "https://b.com"));
    }
}
