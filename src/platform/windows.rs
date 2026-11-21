//! Windows platform probes.

/// Whether this process runs inside WSL/WSLg.
///
/// WSL is a Linux environment, so a native Windows build is never inside it.
/// The probe exists because `app` asks the question from code both platforms
/// share; here the answer is always no.
pub(crate) fn running_under_wsl() -> bool {
    false
}
