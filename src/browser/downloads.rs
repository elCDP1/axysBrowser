use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gtk::Application;
use gtk::gio;
use gtk::prelude::*;
use webkit6::Download;
use webkit6::NetworkSession;

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadStatus {
    InProgress,
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Clone)]
pub struct DownloadEntry {
    pub id: u64,
    pub filename: String,
    pub path: PathBuf,
    pub progress: f64,
    pub status: DownloadStatus,
    download: Download,
}

impl DownloadEntry {
    pub fn is_finished(&self) -> bool {
        self.status != DownloadStatus::InProgress
    }

    /// Current transfer rate in bytes per second, or 0 when unknown.
    pub fn speed_bps(&self) -> f64 {
        if self.status != DownloadStatus::InProgress {
            return 0.0;
        }

        let elapsed = self.download.elapsed_time();

        if elapsed <= 0.0 {
            return 0.0;
        }

        self.download.received_data_length() as f64 / elapsed
    }
}

/// Human-readable byte count with universal (untranslated) unit suffixes.
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;

    let bytes = bytes as f64;

    if bytes < KB {
        format!("{} B", bytes as u64)
    } else if bytes < KB * KB {
        format!("{:.1} KB", bytes / KB)
    } else if bytes < KB * KB * KB {
        format!("{:.1} MB", bytes / (KB * KB))
    } else {
        format!("{:.2} GB", bytes / (KB * KB * KB))
    }
}

/// Human-readable transfer rate, or an empty string when unknown.
pub fn format_speed(bps: f64) -> String {
    if bps <= 0.0 || !bps.is_finite() {
        return String::new();
    }

    format!("{}/s", format_bytes(bps as u64))
}

type DownloadListener = Weak<dyn Fn()>;
type DownloadListeners = Rc<RefCell<Vec<DownloadListener>>>;

/// Tracks downloads started in any `NetworkSession` this manager watches.
/// Every download prompts the user for a destination via a native GTK save
/// dialog (like "Save As" in Chromium), sends a desktop notification when
/// it finishes, and can be cancelled mid-flight.
///
/// Per-window tracking: normal windows share the profile-wide manager (one
/// list everywhere); each private window gets its own manager, so private
/// downloads never appear in normal windows and die with the private
/// window.
#[derive(Clone)]
pub struct DownloadManager {
    entries: Rc<RefCell<Vec<DownloadEntry>>>,
    listeners: DownloadListeners,
    next_id: Rc<RefCell<u64>>,
    application: Application,
    /// Whether finished downloads pop a desktop notification (off in
    /// private windows: the filename would linger in the shell).
    announce: Rc<std::cell::Cell<bool>>,
    /// True for a private window's own manager.
    private: Rc<std::cell::Cell<bool>>,
}

impl DownloadManager {
    pub fn new(application: Application) -> Self {
        Self {
            entries: Rc::new(RefCell::new(Vec::new())),
            listeners: Rc::new(RefCell::new(Vec::new())),
            next_id: Rc::new(RefCell::new(1)),
            application,
            announce: Rc::new(std::cell::Cell::new(true)),
            private: Rc::new(std::cell::Cell::new(false)),
        }
    }

    /// Marks this manager as a private window's: toasts are muted and the
    /// downloads page shows the "files stay on disk" notice.
    pub fn set_private(&self) {
        self.private.set(true);
        self.announce.set(false);
    }

    pub fn is_private(&self) -> bool {
        self.private.get()
    }

    pub fn entries(&self) -> Vec<DownloadEntry> {
        self.entries.borrow().clone()
    }

    /// Registers a callback invoked whenever the download list changes.
    /// Only a weak reference is kept, so the caller must keep the `Rc`
    /// alive itself (e.g. by attaching it to the page/toolbar widget) for
    /// as long as it wants updates.
    pub fn subscribe(&self, callback: &Rc<dyn Fn()>) {
        let mut listeners = self.listeners.borrow_mut();

        listeners.retain(|weak| weak.strong_count() > 0);
        listeners.push(Rc::downgrade(callback));
    }

    fn notify(&self) {
        self.listeners
            .borrow_mut()
            .retain(|weak| weak.strong_count() > 0);

        let listeners = self.listeners.borrow().clone();

        for weak in listeners {
            if let Some(callback) = weak.upgrade() {
                callback();
            }
        }
    }

    pub fn downloads_dir() -> PathBuf {
        let dir = gtk::glib::user_special_dir(gtk::glib::UserDirectory::Downloads)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|home| PathBuf::from(home).join("Downloads"))
            })
            .unwrap_or_else(|| PathBuf::from("."));

        let _ = std::fs::create_dir_all(&dir);

        dir
    }

    /// Opens the system file manager with the given file selected/highlighted.
    pub fn open_containing_folder(path: &Path) {
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));

        launcher.open_containing_folder(None::<&gtk::Window>, gio::Cancellable::NONE, |result| {
            if let Err(error) = result {
                eprintln!("Could not open containing folder: {error}");
            }
        });
    }

    /// Wires up a `NetworkSession` so downloads started in it (triggered by
    /// web content, e.g. clicking a download link or "Save Image As") are
    /// tracked here and prompt the user for a destination.
    pub fn watch(&self, session: &NetworkSession) {
        let manager = self.clone();

        session.connect_download_started(move |_session, download| {
            let id = {
                let mut next = manager.next_id.borrow_mut();
                let id = *next;
                *next += 1;
                id
            };

            manager.entries.borrow_mut().push(DownloadEntry {
                id,
                filename: rust_i18n::t!("downloads.untitled").to_string(),
                path: PathBuf::new(),
                progress: 0.0,
                status: DownloadStatus::InProgress,
                download: download.clone(),
            });

            manager.notify();

            {
                let manager = manager.clone();

                download.connect_decide_destination(move |download, suggested_filename| {
                    let sanitized = sanitize_download_name(suggested_filename);

                    let default_name = if sanitized.is_empty() {
                        rust_i18n::t!("downloads.default_name").to_string()
                    } else {
                        sanitized
                    };

                    let dialog = gtk::FileDialog::builder()
                        .title(rust_i18n::t!("downloads.save_file").as_ref())
                        .initial_folder(&gio::File::for_path(Self::downloads_dir()))
                        .initial_name(&default_name)
                        .build();

                    let manager = manager.clone();
                    let download = download.clone();

                    dialog.save(
                        None::<&gtk::Window>,
                        gio::Cancellable::NONE,
                        move |result| match result {
                            Ok(file) => {
                                let Some(path) = file.path() else {
                                    download.cancel();
                                    return;
                                };

                                let Some(path_str) = path.to_str() else {
                                    download.cancel();
                                    return;
                                };

                                if let Some(entry) =
                                    manager.entries.borrow_mut().iter_mut().find(|e| e.id == id)
                                {
                                    entry.filename = path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_else(|| default_name.clone());

                                    entry.path = path.clone();
                                }

                                manager.notify();

                                download.set_destination(path_str);
                            }

                            Err(_) => {
                                download.cancel();
                            }
                        },
                    );

                    true
                });
            }

            {
                let manager = manager.clone();

                download.connect_estimated_progress_notify(move |download| {
                    let progress = download.estimated_progress();

                    // Progress fires very frequently; rebuilding the toolbar
                    // popover and downloads page on every tick wastes main
                    // loop time during fast downloads. Update the stored value
                    // cheaply, but only notify listeners on visible steps.
                    let should_notify = {
                        if let Some(entry) =
                            manager.entries.borrow_mut().iter_mut().find(|e| e.id == id)
                        {
                            let delta = (progress - entry.progress).abs();

                            entry.progress = progress;

                            delta >= 0.02 || progress >= 1.0
                        } else {
                            false
                        }
                    };

                    if should_notify {
                        manager.notify();
                    }
                });
            }

            {
                let manager = manager.clone();

                download.connect_finished(move |_download| {
                    // `finished` also fires after `failed` / cancellation, so
                    // only announce downloads that really completed.
                    let (filename, completed) = {
                        let mut entries = manager.entries.borrow_mut();

                        let Some(entry) = entries.iter_mut().find(|e| e.id == id) else {
                            return;
                        };

                        let completed = entry.status == DownloadStatus::InProgress;

                        if completed {
                            entry.status = DownloadStatus::Completed;
                            entry.progress = 1.0;
                        }

                        (entry.filename.clone(), completed)
                    };

                    manager.notify();

                    if !completed {
                        return;
                    }

                    // Private windows mute this: the filename would linger
                    // in the desktop notification history.
                    if !manager.announce.get() {
                        return;
                    }

                    // Process-wide counter: normal and private managers
                    // restart their entry ids independently, so reusing the
                    // entry id here would replace unrelated notifications.
                    static NOTIFY_ID: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(1);

                    let tag = NOTIFY_ID.fetch_add(
                        1,
                        std::sync::atomic::Ordering::Relaxed,
                    );

                    let notification =
                        gio::Notification::new(&rust_i18n::t!("downloads.download_complete"));

                    notification.set_body(Some(&filename));

                    manager.application.send_notification(
                        Some(&format!("axys-download-{tag}")),
                        &notification,
                    );
                });
            }

            {
                let manager = manager.clone();

                download.connect_failed(move |_download, error| {
                    if let Some(entry) =
                        manager.entries.borrow_mut().iter_mut().find(|e| e.id == id)
                        && entry.status != DownloadStatus::Cancelled
                    {
                        entry.status = if error.matches(webkit6::DownloadError::CancelledByUser) {
                            DownloadStatus::Cancelled
                        } else {
                            DownloadStatus::Failed(error.to_string())
                        };
                    }

                    manager.notify();
                });
            }
        });
    }

    /// Cancels an in-progress download by id. No-op if it's already finished.
    pub fn cancel(&self, id: u64) {
        let download = {
            let mut entries = self.entries.borrow_mut();

            entries.iter_mut().find(|e| e.id == id).map(|entry| {
                entry.status = DownloadStatus::Cancelled;
                entry.download.clone()
            })
        };

        if let Some(download) = download {
            download.cancel();
        }

        self.notify();
    }

    /// Drops every finished entry (completed, cancelled or failed) from the
    /// list. In-progress downloads are never touched.
    pub fn clear_finished(&self) {
        let changed = {
            let mut entries = self.entries.borrow_mut();

            let before = entries.len();

            entries.retain(|entry| !entry.is_finished());

            entries.len() != before
        };

        if changed {
            self.notify();
        }
    }
}

/// Sanitises a server-suggested filename before using it as the save
/// dialog's initial name: strips directory components (a remote name can
/// contain `../` or be an absolute path), control characters, and caps the
/// length. Returns an empty string when nothing usable remains.
fn sanitize_download_name(suggested: &str) -> String {
    let base = Path::new(suggested)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(suggested);

    let cleaned: String = base.chars().filter(|c| !c.is_control()).collect();

    let cleaned = cleaned.trim();

    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return String::new();
    }

    cleaned.chars().take(255).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_flag_defaults_off_and_mutes_toasts() {
        let manager = DownloadManager::new(gtk::Application::new(
            None::<&str>,
            gtk::gio::ApplicationFlags::empty(),
        ));

        assert!(!manager.is_private());
        assert!(manager.announce.get());

        manager.set_private();

        assert!(manager.is_private());
        assert!(!manager.announce.get());
    }

    #[test]
    fn formats_byte_counts() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn formats_speed_or_empty() {
        assert_eq!(format_speed(0.0), "");
        assert_eq!(format_speed(-5.0), "");
        assert_eq!(format_speed(f64::INFINITY), "");
        assert_eq!(format_speed(1536.0), "1.5 KB/s");
    }

    #[test]
    fn sanitize_name_keeps_plain_name() {
        assert_eq!(sanitize_download_name("report.pdf"), "report.pdf");
    }

    #[test]
    fn sanitize_name_strips_directories() {
        assert_eq!(sanitize_download_name("/etc/passwd"), "passwd");
        assert_eq!(sanitize_download_name("../../secret.txt"), "secret.txt");
        assert_eq!(sanitize_download_name("a/b/c.png"), "c.png");
    }

    #[test]
    fn sanitize_name_rejects_dot_entries() {
        assert_eq!(sanitize_download_name("."), "");
        assert_eq!(sanitize_download_name(".."), "");
        assert_eq!(sanitize_download_name(""), "");
        assert_eq!(sanitize_download_name("   "), "");
    }

    #[test]
    fn sanitize_name_removes_control_characters() {
        assert_eq!(sanitize_download_name("bad\u{0}name\n.txt"), "badname.txt");
    }

    #[test]
    fn sanitize_name_caps_length() {
        let long = "a".repeat(500);

        assert_eq!(sanitize_download_name(&long).chars().count(), 255);
    }
}
