//! Shared filesystem locations and helpers (XDG-aware).

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn xdg_dir(xdg_var: &str, home_fallback: &[&str], last_resort: &str) -> PathBuf {
    if let Ok(xdg) = env::var(xdg_var)
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("axysbrowser");
    }

    if let Ok(home) = env::var("HOME") {
        let mut path = PathBuf::from(home);

        for part in home_fallback {
            path = path.join(part);
        }

        return path.join("axysbrowser");
    }

    PathBuf::from(last_resort)
}

/// Settings, bookmarks, history, cookies.
pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", &[".config"], "axysbrowser")
}

/// Website data (IndexedDB, local storage, …).
pub fn data_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", &[".local", "share"], "axysbrowser-data")
}

/// Disposable caches.
pub fn cache_dir() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", &[".cache"], "axysbrowser-cache")
}

/// Writes through a temporary file + rename so a crash never leaves a
/// half-written file behind.
pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path has no parent directory",
        ));
    };

    fs::create_dir_all(parent)?;

    let temporary = path.with_extension("tmp");

    fs::write(&temporary, contents)?;
    fs::rename(temporary, path)
}
