//! Platform facts that are not engine facts.
//!
//! `browser::engine` answers "which browser engine runs here". This module
//! answers "which operating system is this" for the handful of decisions that
//! are neither: how a path is spelled, and whether we are inside WSLg, whose
//! compositor changes how wheel input and rendering behave.
//!
//! Keeping them out of the engine is what lets `app` and `ui` stay the same
//! file on both platforms while each backend answers only for the engine.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::running_under_wsl;
#[cfg(windows)]
pub(crate) use windows::running_under_wsl;
