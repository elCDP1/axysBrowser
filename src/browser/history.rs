use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_HISTORY_ENTRIES: usize = 10_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub title: String,
    pub url: String,
    pub visited_at: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct HistoryStore {
    entries: Vec<HistoryEntry>,
}

type Listener = Weak<dyn Fn()>;
type Listeners = Rc<RefCell<Vec<Listener>>>;

#[derive(Clone)]
pub struct HistoryManager {
    entries: Rc<RefCell<Vec<HistoryEntry>>>,
    path: PathBuf,
    listeners: Listeners,
}

impl HistoryManager {
    pub fn load() -> Self {
        let path = Self::storage_path();

        let mut entries = fs::read_to_string(&path)
            .ok()
            .and_then(|contents| {
                toml::from_str::<HistoryStore>(&contents)
                    .ok()
                    .map(|store| store.entries)
            })
            .unwrap_or_default();

        if entries.len() > MAX_HISTORY_ENTRIES {
            let remove_count = entries.len() - MAX_HISTORY_ENTRIES;

            entries.drain(0..remove_count);
        }

        Self {
            entries: Rc::new(RefCell::new(entries)),
            path,
            listeners: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn add_visit(&self, title: impl Into<String>, url: impl Into<String>) {
        let url = url.into();

        if !Self::is_recordable_url(&url) {
            return;
        }

        let title = title.into();

        let visited_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());

        let changed = {
            let mut entries = self.entries.borrow_mut();

            if let Some(last) = entries.last()
                && last.url == url
                && visited_at.saturating_sub(last.visited_at) < 2
            {
                false
            } else {
                entries.push(HistoryEntry {
                    title: sanitize_title(title, &url),
                    url,
                    visited_at,
                });

                if entries.len() > MAX_HISTORY_ENTRIES {
                    let remove_count = entries.len() - MAX_HISTORY_ENTRIES;

                    entries.drain(0..remove_count);
                }

                true
            }
        };

        if changed {
            self.persist_and_notify();
        }
    }

    pub fn entries(&self) -> Vec<HistoryEntry> {
        self.entries.borrow().iter().rev().cloned().collect()
    }

    pub fn search(&self, query: &str) -> Vec<HistoryEntry> {
        let query = query.trim().to_ascii_lowercase();

        if query.is_empty() {
            return self.entries();
        }

        self.entries
            .borrow()
            .iter()
            .rev()
            .filter(|entry| {
                entry.title.to_ascii_lowercase().contains(&query)
                    || entry.url.to_ascii_lowercase().contains(&query)
            })
            .cloned()
            .collect()
    }

    pub fn remove_url(&self, url: &str) -> bool {
        let removed = {
            let mut entries = self.entries.borrow_mut();

            let before = entries.len();

            entries.retain(|entry| entry.url != url);

            entries.len() != before
        };

        if removed {
            self.persist_and_notify();
        }

        removed
    }

    pub fn clear(&self) {
        let changed = {
            let mut entries = self.entries.borrow_mut();

            if entries.is_empty() {
                false
            } else {
                entries.clear();
                true
            }
        };

        if changed {
            self.persist_and_notify();
        }
    }

    /// Deletes every entry older than `max_age_secs`. Returns how many were
    /// removed. Used by the manual "delete older than …" tools.
    pub fn remove_older_than(&self, max_age_secs: u64) -> usize {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());

        let removed = {
            let mut entries = self.entries.borrow_mut();

            let before = entries.len();

            entries.retain(|entry| now.saturating_sub(entry.visited_at) <= max_age_secs);

            before - entries.len()
        };

        if removed > 0 {
            self.persist_and_notify();
        }

        removed
    }

    pub fn subscribe(&self, callback: &Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(Rc::downgrade(callback));
    }

    fn persist_and_notify(&self) {
        if let Err(error) = self.save() {
            eprintln!("Failed to save browsing history: {error}");
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

    fn is_recordable_url(url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }

    fn storage_path() -> PathBuf {
        crate::paths::config_dir().join("history.toml")
    }

    fn save(&self) -> io::Result<()> {
        let entries = self.entries.borrow();

        let store = HistoryStore {
            entries: entries.clone(),
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
    use super::*;

    fn test_manager(name: &str) -> HistoryManager {
        HistoryManager {
            entries: Rc::new(RefCell::new(Vec::new())),
            path: std::env::temp_dir().join(format!("axys-history-test-{name}.toml")),
            listeners: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn entry_at(url: &str, visited_at: u64) -> HistoryEntry {
        HistoryEntry {
            title: url.to_string(),
            url: url.to_string(),
            visited_at,
        }
    }

    #[test]
    fn remove_older_than_keeps_recent_entries() {
        let manager = test_manager("purge");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());

        manager.entries.borrow_mut().push(entry_at("https://old.example", now - 100_000));
        manager.entries.borrow_mut().push(entry_at("https://new.example", now - 10));

        let removed = manager.remove_older_than(3_600);

        assert_eq!(removed, 1);

        let remaining = manager.entries();

        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].url, "https://new.example");
    }

    #[test]
    fn clear_empties_everything() {
        let manager = test_manager("clear");

        manager.entries.borrow_mut().push(entry_at("https://a.example", 1));
        manager.entries.borrow_mut().push(entry_at("https://b.example", 2));

        manager.clear();

        assert!(manager.entries().is_empty());
    }

    #[test]
    fn non_web_urls_are_never_recordable() {
        // Internal pages, blanks and local files must never reach history
        // (this is what keeps private-adjacent pages out of it too).
        for url in [
            "axys://newtab",
            "axys://privacy",
            "axys://settings",
            "about:blank",
            "file:///home/user/page.html",
            "data:text/html,hi",
        ] {
            let manager = test_manager("recordable");

            manager.add_visit("title", url);

            assert!(
                manager.entries().is_empty(),
                "{url} must not be recorded"
            );
        }
    }
}
