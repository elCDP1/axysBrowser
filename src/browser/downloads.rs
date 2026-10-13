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

/// Tracks downloads started in any `NetworkSession` this manager watches
/// (normal browsing and, separately, each private window's ephemeral
/// session). Every download prompts the user for a destination via a
/// native GTK save dialog (like "Save As" in Chromium), sends a desktop
/// notification when it finishes, and can be cancelled mid-flight. Shared
/// via `AppState`, so every open `axys://downloads` page and the toolbar's
/// downloads button across all windows reflect the same list.
#[derive(Clone)]
pub struct DownloadManager {
    entries: Rc<RefCell<Vec<DownloadEntry>>>,
    listeners: DownloadListeners,
    next_id: Rc<RefCell<u64>>,
    application: Application,
}

impl DownloadManager {
    pub fn new(application: Application) -> Self {
        Self {
            entries: Rc::new(RefCell::new(Vec::new())),
            listeners: Rc::new(RefCell::new(Vec::new())),
            next_id: Rc::new(RefCell::new(1)),
            application,
        }
    }

    pub fn entries(&self) -> Vec<DownloadEntry> {
        self.entries.borrow().clone()
    }

    /// Registers a callback invoked whenever the download list changes.
    /// Only a weak reference is kept, so the caller must keep the `Rc`
    /// alive itself (e.g. by attaching it to the page/toolbar widget) for
    /// as long as it wants updates.
    pub fn subscribe(&self, callback: &Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(Rc::downgrade(callback));
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
        let dir = if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join("Downloads")
        } else {
            PathBuf::from(".")
        };

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
                filename: "Download".to_string(),
                path: PathBuf::new(),
                progress: 0.0,
                status: DownloadStatus::InProgress,
                download: download.clone(),
            });

            manager.notify();

            {
                let manager = manager.clone();

                download.connect_decide_destination(move |download, suggested_filename| {
                    let default_name = if suggested_filename.trim().is_empty() {
                        "download".to_string()
                    } else {
                        suggested_filename.to_string()
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
                    let filename = {
                        let mut entries = manager.entries.borrow_mut();

                        let Some(entry) = entries.iter_mut().find(|e| e.id == id) else {
                            return;
                        };

                        if entry.status == DownloadStatus::InProgress {
                            entry.status = DownloadStatus::Completed;
                            entry.progress = 1.0;
                        }

                        entry.filename.clone()
                    };

                    manager.notify();

                    let notification =
                        gio::Notification::new(&rust_i18n::t!("downloads.download_complete"));

                    notification.set_body(Some(&filename));

                    manager
                        .application
                        .send_notification(Some(&format!("axys-download-{id}")), &notification);
                });
            }

            {
                let manager = manager.clone();

                download.connect_failed(move |_download, error| {
                    if let Some(entry) =
                        manager.entries.borrow_mut().iter_mut().find(|e| e.id == id)
                        && entry.status != DownloadStatus::Cancelled
                    {
                        entry.status = DownloadStatus::Failed(error.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
