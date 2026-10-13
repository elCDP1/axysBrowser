use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
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
        config_directory().join("history.toml")
    }

    fn save(&self) -> io::Result<()> {
        let entries = self.entries.borrow();

        let store = HistoryStore {
            entries: entries.clone(),
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

fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "history path has no parent",
        ));
    };

    fs::create_dir_all(parent)?;

    let temporary = path.with_extension("tmp");

    fs::write(&temporary, contents)?;

    fs::rename(temporary, path)
}
