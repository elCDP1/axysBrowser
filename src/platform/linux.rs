//! Linux platform probes.

/// Whether this process runs inside WSL/WSLg.
///
/// WSLg delivers high-resolution wheel deltas on top of an already fine-grained
/// device, and its rendering path is not the native one, so the browser is
/// configured differently there. The environment variables cover WSL launches;
/// `/proc/version` catches the cases where they are not exported.
pub(crate) fn running_under_wsl() -> bool {
    if std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok() {
        return true;
    }

    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        let lower = version.to_ascii_lowercase();

        if lower.contains("microsoft") || lower.contains("wsl") {
            return true;
        }
    }

    false
}
