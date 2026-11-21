pub mod address;
pub mod bookmarks;
pub mod downloads;
pub mod engine;
pub mod history;
/// Remembered per-origin permission decisions.
///
/// Only a backend that can ask a page for a decision has anything to remember:
/// the WebKit delegate does, Servo 0.7 cannot answer a permission request at
/// all, so the module does not exist there and neither does its settings row.
#[cfg(target_os = "linux")]
pub mod permissions;
pub mod tabs;
pub mod util;
