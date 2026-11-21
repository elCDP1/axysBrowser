//! The seam's download handle, on the Servo backend.
//!
//! Servo 0.7's embedder API has no download interception: `WebResourceLoad`
//! exposes subresource interception, not transfers, and there is no object
//! with a destination, a progress figure or a cancel. The seam still needs the
//! type because `browser/downloads.rs` is written against it, so it is
//! uninhabited: no value can exist, `DownloadManager::watch` is not compiled on
//! this backend, and every method is total over zero inhabitants.
//!
//! Only the half the interface reads is here — a cancel, a byte count, a
//! speed and a progress figure, all of them for the downloads page. Everything
//! a real transfer would be asked for (where to save, when it progressed, when
//! it ended) belongs to the interception, and the interception is where the
//! engine cannot produce one.
//!
//! This is the honest shape for "the engine cannot produce one". The day Servo
//! grows downloads, this file and the session hook change and nothing else.

/// One transfer. Uninhabited: this backend never starts one.
#[derive(Clone)]
pub enum Download {}

impl Download {
    /// The only possible body for a type with no values.
    fn impossible(&self) -> ! {
        match *self {}
    }

    /// Stops the transfer. The engine then reports it as cancelled.
    pub fn cancel(&self) {
        self.impossible()
    }

    pub fn received_bytes(&self) -> u64 {
        self.impossible()
    }

    /// Seconds since the transfer started, for the speed display.
    pub fn elapsed_seconds(&self) -> f64 {
        self.impossible()
    }
}
