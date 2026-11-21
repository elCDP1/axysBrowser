//! The seam's download handle, backed by WebKitGTK.
//!
//! The download manager (`browser/downloads.rs`) tracks transfers and asks the
//! user for a destination; this is everything it needs from a transfer, in
//! neutral terms: bytes, progress, where to save, and the three events.

use gtk::prelude::*;
use webkit6::{Download as WebkitDownload, DownloadError};

/// One transfer, as the interface sees it.
#[derive(Clone)]
pub struct Download {
    inner: WebkitDownload,
}

impl Download {
    pub(crate) fn from_webkit(inner: WebkitDownload) -> Self {
        Self { inner }
    }

    /// Stops the transfer. The engine then reports it as cancelled.
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    pub fn received_bytes(&self) -> u64 {
        self.inner.received_data_length()
    }

    /// Seconds since the transfer started, for the speed display.
    pub fn elapsed_seconds(&self) -> f64 {
        self.inner.elapsed_time()
    }

    /// 0.0 … 1.0, or 0.0 while the engine has no estimate.
    pub fn progress(&self) -> f64 {
        self.inner.estimated_progress()
    }

    /// The window the transfer belongs to, for the save dialog's parent. The
    /// active window may have changed while the destination was being chosen,
    /// so this is resolved from the page that started the download.
    pub fn parent_window(&self) -> Option<gtk::Window> {
        self.inner
            .web_view()
            .and_then(|view| view.root())
            .and_then(|root| root.downcast::<gtk::Window>().ok())
    }

    /// The engine refuses to replace an existing file unless the embedder
    /// says the user already confirmed it.
    pub fn set_allow_overwrite(&self, allow: bool) {
        self.inner.set_allow_overwrite(allow);
    }

    /// Where to save, as a URI (the engine expects one, not a path: it is what
    /// encodes spaces and non-ASCII characters safely).
    pub fn set_destination(&self, uri: &str) {
        self.inner.set_destination(uri);
    }

    /// Called once, with the engine's suggested file name, before the transfer
    /// starts. The request is claimed either way: an embedder that does not
    /// want the download cancels it from inside.
    pub fn on_decide_destination(&self, f: impl Fn(Download, &str) + 'static) {
        self.inner
            .connect_decide_destination(move |download, suggested_filename| {
                f(Download::from_webkit(download.clone()), suggested_filename);

                true
            });
    }

    /// Progress ticks. They are frequent; the manager throttles the redraws.
    pub fn on_progress(&self, f: impl Fn(Download) + 'static) {
        self.inner
            .connect_estimated_progress_notify(move |download| {
                f(Download::from_webkit(download.clone()));
            });
    }

    /// The transfer ended, whether it succeeded, failed or was cancelled.
    pub fn on_finished(&self, f: impl Fn() + 'static) {
        self.inner.connect_finished(move |_| f());
    }

    /// The transfer failed. `cancelled` distinguishes the user's own
    /// cancellation from a network or disk error.
    pub fn on_failed(&self, f: impl Fn(&str, bool) + 'static) {
        self.inner.connect_failed(move |_, error| {
            f(
                &error.to_string(),
                error.matches(DownloadError::CancelledByUser),
            );
        });
    }
}
