use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_HISTORY_ENTRIES: usize = 10_000;

/// Visits are written to disk at most this often. Serializing and syncing
/// the whole (up to 10 000 entries) file takes tens of milliseconds, which
/// used to be paid on the UI thread after *every* page load.
const SAVE_DELAY: Duration = Duration::from_secs(2);

/// After a failed deferred write the timer is re-armed this many times, with
/// the delay doubling each time (4, 8, 16, 32 s). Past that it gives up until
/// the next visit or the shutdown flush tries again, so a persistent failure
/// (full disk, read-only profile) never turns into a retry loop.
const MAX_SAVE_RETRIES: u32 = 4;

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

/// Borrowing twin of [`HistoryStore`] used to write: same TOML, no clone of
/// the whole history per save.
#[derive(Serialize)]
struct HistoryStoreRef<'a> {
    entries: &'a [HistoryEntry],
}

/// Write-coalescing state shared by every clone of a manager.
#[derive(Default)]
struct PendingSave {
    /// Entries changed since the last write.
    dirty: Cell<bool>,
    /// A deferred write is already scheduled.
    armed: Cell<bool>,
    /// Consecutive failed deferred writes; reset by any successful write.
    failures: Cell<u32>,
}

type Listener = Weak<dyn Fn()>;
type Listeners = Rc<RefCell<Vec<Listener>>>;

#[derive(Clone)]
pub struct HistoryManager {
    entries: Rc<RefCell<Vec<HistoryEntry>>>,
    /// `None` = ephemeral (private windows): kept in memory for the session,
    /// never read from or written to disk.
    path: Option<PathBuf>,
    listeners: Listeners,
    pending: Rc<PendingSave>,
}

impl HistoryManager {
    pub fn load() -> Self {
        Self::load_from(Self::storage_path())
    }

    /// Starts an empty, non-persistent manager. Used by private windows so
    /// their `axys://history` page shows only that session's visits and
    /// nothing ever touches the on-disk history.
    pub fn ephemeral() -> Self {
        Self {
            entries: Rc::new(RefCell::new(Vec::new())),
            path: None,
            listeners: Rc::new(RefCell::new(Vec::new())),
            pending: Rc::default(),
        }
    }

    fn load_from(path: PathBuf) -> Self {
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
            path: Some(path),
            listeners: Rc::new(RefCell::new(Vec::new())),
            pending: Rc::default(),
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
                && visited_at >= last.visited_at
                && visited_at - last.visited_at < 2
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
            self.schedule_save();
            self.notify();
        }
    }

    /// Newest-first matches, capped at `limit` rows so pages never clone
    /// and render thousands of widgets per keystroke.
    pub fn search_limited(&self, query: &str, limit: usize) -> Vec<HistoryEntry> {
        // Unicode-aware (not `to_ascii_lowercase`): Spanish titles such as
        // "España" must match a lowercase query.
        let query = query.trim().to_lowercase();

        if query.is_empty() {
            return self
                .entries
                .borrow()
                .iter()
                .rev()
                .take(limit)
                .cloned()
                .collect();
        }

        self.entries
            .borrow()
            .iter()
            .rev()
            .filter(|entry| {
                entry.title.to_lowercase().contains(&query)
                    || entry.url.to_lowercase().contains(&query)
            })
            .take(limit)
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

    /// Forgets every entry and rewrites the history file. Always touches
    /// the file (even when memory is already empty) so entries that could
    /// not be parsed at load time never survive a "clear"; the error is
    /// returned so callers can avoid claiming success.
    pub fn clear(&self) -> io::Result<()> {
        self.entries.borrow_mut().clear();

        let result = self.save();

        self.notify();

        result
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
        let mut listeners = self.listeners.borrow_mut();

        listeners.retain(|listener| listener.strong_count() > 0);
        listeners.push(Rc::downgrade(callback));
    }

    /// Persists soon, coalescing bursts of visits into one write. When no
    /// main loop owns this thread (unit tests, teardown) it writes at once.
    fn schedule_save(&self) {
        self.schedule_save_with(gtk::glib::MainContext::default().is_owner(), |manager| {
            gtk::glib::timeout_add_local_once(SAVE_DELAY, move || manager.on_save_timer());
        });
    }

    /// `defer`: a main loop will run `arm`'s timer. Split out so the
    /// coalescing can be tested without a running GLib main loop.
    fn schedule_save_with(&self, defer: bool, arm: impl FnOnce(HistoryManager)) {
        if self.path.is_none() {
            return;
        }

        self.pending.dirty.set(true);

        if !defer {
            self.flush();

            return;
        }

        if !self.pending.armed.replace(true) {
            arm(self.clone());
        }
    }

    fn on_save_timer(&self) {
        self.on_save_timer_with(|manager, delay| {
            gtk::glib::timeout_add_local_once(delay, move || manager.on_save_timer());
        });
    }

    /// Runs the deferred write; if it failed, `rearm` schedules the retry
    /// (see [`MAX_SAVE_RETRIES`]). Split out so the backoff can be tested
    /// without a running GLib main loop.
    fn on_save_timer_with(&self, rearm: impl FnOnce(HistoryManager, Duration)) {
        self.pending.armed.set(false);

        self.flush();

        if let Some(delay) = self.retry_delay() {
            self.pending.armed.set(true);

            rearm(self.clone(), delay);
        }
    }

    /// `Some(delay)` while the last writes failed and retries remain.
    fn retry_delay(&self) -> Option<Duration> {
        let failures = self.pending.failures.get();

        (self.pending.dirty.get() && (1..=MAX_SAVE_RETRIES).contains(&failures))
            .then(|| SAVE_DELAY * 2u32.pow(failures))
    }

    /// Writes any deferred change now. Called on application shutdown so a
    /// normal exit never loses the last visits; a no-op when nothing is
    /// pending. A failed write keeps the change dirty so a later timer or
    /// flush can retry it.
    pub fn flush(&self) {
        if !self.pending.dirty.get() {
            return;
        }

        if let Err(error) = self.save() {
            self.pending
                .failures
                .set(self.pending.failures.get().saturating_add(1));

            eprintln!("Failed to save browsing history: {error}");
        }
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
        // Ephemeral managers (private windows) deliberately never persist.
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };

        // Mark the state dirty while writing: if serialization or the atomic
        // rename fails, a later timer or shutdown flush retries it.
        self.pending.dirty.set(true);

        let entries = self.entries.borrow();

        let store = HistoryStoreRef { entries: &entries };

        let contents = toml::to_string_pretty(&store).map_err(io::Error::other)?;

        crate::paths::atomic_write(path, &contents)?;

        self.pending.dirty.set(false);
        self.pending.failures.set(0);

        Ok(())
    }
}

fn sanitize_title(title: String, url: &str) -> String {
    let collapsed: String = title.split_whitespace().collect::<Vec<_>>().join(" ");

    if collapsed.trim().is_empty() {
        url.to_string()
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager(name: &str) -> HistoryManager {
        HistoryManager {
            entries: Rc::new(RefCell::new(Vec::new())),
            path: Some(std::env::temp_dir().join(format!("axys-history-test-{name}.toml"))),
            listeners: Rc::new(RefCell::new(Vec::new())),
            pending: Rc::default(),
        }
    }

    fn test_path(manager: &HistoryManager) -> &std::path::Path {
        manager.path.as_deref().expect("test manager persists")
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

        manager
            .entries
            .borrow_mut()
            .push(entry_at("https://old.example", now - 100_000));
        manager
            .entries
            .borrow_mut()
            .push(entry_at("https://new.example", now - 10));

        let removed = manager.remove_older_than(3_600);

        assert_eq!(removed, 1);

        let remaining = manager.search_limited("", usize::MAX);

        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].url, "https://new.example");
    }

    #[test]
    fn search_is_case_insensitive_for_accented_text() {
        let manager = test_manager("search-unicode");

        manager.entries.borrow_mut().push(HistoryEntry {
            title: "España".to_string(),
            url: "https://example.com/ESPANA".to_string(),
            visited_at: 1,
        });

        assert_eq!(manager.search_limited("españa", usize::MAX).len(), 1);
        assert_eq!(manager.search_limited("ESPAÑA", usize::MAX).len(), 1);
        assert!(manager.search_limited("portugal", usize::MAX).is_empty());
    }

    #[test]
    fn clear_empties_everything() {
        let manager = test_manager("clear");

        manager
            .entries
            .borrow_mut()
            .push(entry_at("https://a.example", 1));
        manager
            .entries
            .borrow_mut()
            .push(entry_at("https://b.example", 2));

        assert!(manager.clear().is_ok());

        assert!(manager.search_limited("", usize::MAX).is_empty());
    }

    #[test]
    fn clear_wipes_unparsed_file_and_reports_write_errors() {
        // A corrupt file loads as empty in memory but still holds old URLs
        // on disk: clearing must overwrite it, not skip the write.
        let manager = test_manager("clear-corrupt");

        let path = test_path(&manager).to_path_buf();

        fs::write(&path, "entries = [corrupt https://secret.example").unwrap();

        assert!(manager.clear().is_ok());

        let text = fs::read_to_string(&path).unwrap();

        assert!(!text.contains("secret.example"));

        let _ = fs::remove_file(&path);

        // An unwritable location must surface as an error, not success.
        let broken = HistoryManager {
            entries: Rc::new(RefCell::new(Vec::new())),
            path: Some(PathBuf::from("/proc/axys-nonexistent/history.toml")),
            listeners: Rc::new(RefCell::new(Vec::new())),
            pending: Rc::default(),
        };

        assert!(broken.clear().is_err());
    }

    #[test]
    fn history_persists_across_reload() {
        let path = std::env::temp_dir().join("axys-history-persist-test.toml");

        let _ = fs::remove_file(&path);

        let manager = HistoryManager::load_from(path.clone());

        manager.add_visit("Example", "https://example.com/page");
        manager.add_visit("Other", "https://other.example/x");

        // A fresh manager reading the same file must see both visits, newest
        // first, with their titles intact.
        let reloaded = HistoryManager::load_from(path.clone());

        let entries = reloaded.search_limited("", usize::MAX);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://other.example/x");
        assert_eq!(entries[0].title, "Other");
        assert_eq!(entries[1].url, "https://example.com/page");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn ephemeral_history_is_never_persisted() {
        // Private windows use an ephemeral manager: visits are visible for
        // the session but no history file is ever created or consulted.
        let manager = HistoryManager::ephemeral();

        assert!(manager.path.is_none());

        manager.add_visit("Secret", "https://private.example/secret");

        let entries = manager.search_limited("", usize::MAX);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "https://private.example/secret");

        // Saving (via any mutation) is a no-op that cannot fail or write.
        assert!(manager.clear().is_ok());
        assert!(manager.search_limited("", usize::MAX).is_empty());
    }

    #[test]
    fn ephemeral_and_persistent_history_do_not_share_entries() {
        let manager = HistoryManager::ephemeral();

        manager.add_visit("Private", "https://private.example/a");

        // The persistent store is a different instance; a private visit must
        // not leak into it.
        let persistent = test_manager("isolation");

        assert!(persistent.search_limited("", usize::MAX).is_empty());
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
                manager.search_limited("", usize::MAX).is_empty(),
                "{url} must not be recorded"
            );
        }
    }

    #[test]
    fn deferred_visits_coalesce_into_a_single_write() {
        let manager = test_manager("coalesce");
        let path = test_path(&manager).to_path_buf();
        let _ = fs::remove_file(&path);
        let armed = Cell::new(0);

        for i in 0..1000 {
            manager
                .entries
                .borrow_mut()
                .push(entry_at(&format!("https://example.com/{i}"), i));

            manager.schedule_save_with(true, |_| armed.set(armed.get() + 1));
        }

        assert_eq!(armed.get(), 1, "one timer for the whole burst");
        assert!(!path.exists(), "nothing is written while deferred");

        manager.flush();

        let reloaded = HistoryManager::load_from(path.clone());
        assert_eq!(reloaded.entries.borrow().len(), 1000);

        // Idempotent: a second flush (or the late timer) writes nothing.
        fs::remove_file(&path).unwrap();
        manager.flush();
        manager.on_save_timer();
        assert!(!path.exists());
    }

    #[test]
    fn immediate_privacy_writes_supersede_a_pending_one() {
        let manager = test_manager("supersede");
        let path = test_path(&manager).to_path_buf();
        let _ = fs::remove_file(&path);

        manager
            .entries
            .borrow_mut()
            .push(entry_at("https://example.com/a", 1));
        manager.schedule_save_with(true, |_| {});

        assert!(!path.exists());

        manager.clear().unwrap();

        assert!(path.exists(), "clear writes immediately");

        fs::remove_file(&path).unwrap();
        manager.flush();

        assert!(!path.exists(), "the pending write was superseded");
    }

    #[test]
    fn without_a_main_loop_visits_are_written_immediately() {
        let manager = test_manager("immediate");
        let path = test_path(&manager).to_path_buf();
        let _ = fs::remove_file(&path);

        manager.add_visit("Title", "https://example.com/x");

        assert_eq!(
            HistoryManager::load_from(path.clone())
                .entries
                .borrow()
                .len(),
            1
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn failed_write_keeps_the_change_dirty_for_a_retry() {
        let manager = test_manager("retry");
        let path = test_path(&manager).to_path_buf();
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&path);

        manager
            .entries
            .borrow_mut()
            .push(entry_at("https://example.com/retry", 1));
        manager.schedule_save_with(true, |_| {});

        // A directory at the destination makes the atomic rename fail.
        fs::create_dir(&path).unwrap();
        manager.flush();

        assert!(
            manager.pending.dirty.get(),
            "a failed write must stay dirty so it can be retried"
        );

        fs::remove_dir(&path).unwrap();
        manager.flush();

        assert!(!manager.pending.dirty.get());
        assert_eq!(
            HistoryManager::load_from(path.clone())
                .entries
                .borrow()
                .len(),
            1
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn ephemeral_manager_never_schedules_a_write() {
        let manager = HistoryManager::ephemeral();
        let armed = Cell::new(0);

        manager.schedule_save_with(true, |_| armed.set(armed.get() + 1));

        assert_eq!(armed.get(), 0);
    }

    #[test]
    fn borrowed_store_serializes_like_the_owned_one() {
        let entries = vec![
            entry_at("https://example.com/a", 1),
            entry_at("https://example.com/b", 2),
        ];

        let owned = toml::to_string_pretty(&HistoryStore {
            entries: entries.clone(),
        })
        .unwrap();

        let borrowed = toml::to_string_pretty(&HistoryStoreRef { entries: &entries }).unwrap();

        assert_eq!(owned, borrowed);
    }

    /// Makes writes to `path` fail (a directory sits where the file goes).
    fn block_path(path: &std::path::Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(path);

        fs::create_dir(path).unwrap();
    }

    #[test]
    fn failed_deferred_write_is_retried_with_bounded_backoff() {
        let manager = test_manager("backoff");
        let path = test_path(&manager).to_path_buf();

        block_path(&path);

        manager
            .entries
            .borrow_mut()
            .push(entry_at("https://example.com/backoff", 1));
        manager.schedule_save_with(true, |_| {});

        let mut delays = Vec::new();

        // The original attempt plus MAX_SAVE_RETRIES retries, then it stops.
        for _ in 0..=MAX_SAVE_RETRIES {
            manager.on_save_timer_with(|_, delay| delays.push(delay));
        }

        assert_eq!(
            delays,
            [4, 8, 16, 32].map(Duration::from_secs),
            "doubling delays, one per allowed retry"
        );
        assert!(
            !manager.pending.armed.get(),
            "no timer is left armed after giving up"
        );
        assert!(manager.pending.dirty.get(), "the change is still pending");

        // Once the disk recovers, the next attempt saves and resets.
        fs::remove_dir(&path).unwrap();
        manager.flush();

        assert!(!manager.pending.dirty.get());
        assert_eq!(manager.pending.failures.get(), 0);
        assert_eq!(
            HistoryManager::load_from(path.clone())
                .entries
                .borrow()
                .len(),
            1
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_successful_timer_write_does_not_rearm() {
        let manager = test_manager("no-rearm");
        let path = test_path(&manager).to_path_buf();
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&path);

        manager
            .entries
            .borrow_mut()
            .push(entry_at("https://example.com/ok", 1));
        manager.schedule_save_with(true, |_| {});

        let mut rearmed = 0;
        manager.on_save_timer_with(|_, _| rearmed += 1);

        assert_eq!(rearmed, 0);
        assert!(!manager.pending.armed.get());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_new_visit_after_giving_up_arms_the_timer_again() {
        let manager = test_manager("rearm-after-giveup");
        let path = test_path(&manager).to_path_buf();

        block_path(&path);

        manager
            .entries
            .borrow_mut()
            .push(entry_at("https://example.com/a", 1));
        manager.schedule_save_with(true, |_| {});

        for _ in 0..=MAX_SAVE_RETRIES {
            manager.on_save_timer_with(|_, _| {});
        }

        let armed = Cell::new(0);
        manager.schedule_save_with(true, |_| armed.set(armed.get() + 1));

        assert_eq!(armed.get(), 1, "a fresh visit retries without waiting");

        let _ = fs::remove_dir_all(&path);
    }
}
