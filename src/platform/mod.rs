//! Platform facts that are not engine facts.
//!
//! `browser::engine` answers "which browser engine runs here". This module
//! answers "which operating system is this" for the handful of decisions that
//! are neither: how a path is spelled, and whether we are inside WSLg, whose
//! compositor changes how wheel input and rendering behave.
//!
//! Keeping them out of the engine is what lets `app` and `ui` stay the same
//! file on both platforms while each backend answers only for the engine.
//!
//! Every question left here belongs to Linux. A native Windows process is
//! never inside WSL, and the startup probes that need to know are gated to
//! Linux (`app::linux`), so this module has no Windows half.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub(crate) use linux::running_under_wsl;
