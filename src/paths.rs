//! Shared filesystem locations and helpers (XDG-aware).

use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
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

/// Creates the browser's config, data and cache directories owner-only
/// (0700) and tightens them if an older version created them world-readable.
/// Cookies, history and bookmarks live below these, so keeping the top-level
/// directories private protects every file inside.
pub fn secure_dirs() {
    for dir in [config_dir(), data_dir(), cache_dir()] {
        if let Err(error) = fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .and_then(|()| fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)))
        {
            eprintln!("Failed to secure {}: {error}", dir.display());
        }
    }
}

/// Writes through a temporary file + rename so a crash never leaves a
/// half-written file behind. Files are created owner-only (0600).
pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path has no parent directory",
        ));
    };

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;

    // Unique per filename (not just per extension) and per process, so
    // `foo.toml` and `foo.json` never share a temp file.
    let temporary = match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => path.with_file_name(format!(".{name}.{}.tmp", std::process::id())),
        None => path.with_extension("tmp"),
    };

    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;

        file.write_all(contents.as_bytes())?;

        // Flush to disk before the rename: otherwise a crash right after
        // the rename could leave the real file pointing at zero/partial
        // contents, silently losing history/bookmarks/permissions.
        file.sync_all()?;
    }

    fs::rename(&temporary, path)?;

    // Best effort: also persist the directory entry (a failed directory
    // fsync must not fail the save on filesystems that don't support it).
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("axys-paths-{}-{name}", std::process::id()))
    }

    #[test]
    fn atomic_write_creates_parent_and_uses_owner_only_modes() {
        let dir = unique_dir("create");
        let _ = fs::remove_dir_all(&dir);

        let path = dir.join("nested").join("settings.toml");

        atomic_write(&path, "hello").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");

        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600, "data files must not be world-readable");

        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_overwrites_and_leaves_no_temp_file() {
        let dir = unique_dir("overwrite");
        let _ = fs::remove_dir_all(&dir);

        fs::create_dir_all(&dir).unwrap();

        let path = dir.join("data.toml");

        fs::write(&path, "old").unwrap();

        atomic_write(&path, "new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");

        let leftovers: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();

        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_rejects_paths_without_a_parent() {
        assert!(atomic_write(Path::new(""), "x").is_err());
    }
}
