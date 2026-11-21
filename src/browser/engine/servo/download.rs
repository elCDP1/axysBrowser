//! The seam's download handle, on the Servo backend.
//!
//! Servo 0.7's embedder API has no download interception: `WebResourceLoad`
//! exposes subresource interception, not transfers, and there is no object
//! with a destination, a progress figure or a cancel. The seam still needs the
//! type because `browser/downloads.rs` is written against it, so it is
//! uninhabited: no value can exist, `Session::on_download_started` never calls
//! its callback, and every method is total over zero inhabitants.
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

    /// 0.0 … 1.0, or 0.0 while the engine has no estimate.
    pub fn progress(&self) -> f64 {
        self.impossible()
    }

    /// The window the transfer belongs to, for the save dialog's parent.
    pub fn parent_window(&self) -> Option<gtk::Window> {
        self.impossible()
    }

    pub fn set_allow_overwrite(&self, _allow: bool) {
        self.impossible()
    }

    /// Where to save, as a URI.
    pub fn set_destination(&self, _uri: &str) {
        self.impossible()
    }

    pub fn on_decide_destination(&self, _f: impl Fn(Download, &str) + 'static) {
        self.impossible()
    }

    pub fn on_progress(&self, _f: impl Fn(Download) + 'static) {
        self.impossible()
    }

    pub fn on_finished(&self, _f: impl Fn() + 'static) {
        self.impossible()
    }

    pub fn on_failed(&self, _f: impl Fn(&str, bool) + 'static) {
        self.impossible()
    }
}
