//! Shared filesystem locations and helpers (XDG-aware).

use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Resolves an XDG directory from raw values, so the fallback chain can be
/// tested without mutating the process environment.
///
/// XDG Base Directory: a relative or empty `XDG_*_HOME` is invalid and must
/// be ignored. Accepting one scattered the profile next to whatever
/// directory the browser happened to be launched from.
fn xdg_dir_from(
    xdg: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    home_fallback: &[&str],
    last_resort: &str,
) -> PathBuf {
    if let Some(xdg) = xdg {
        let path = Path::new(xdg);

        if !xdg.is_empty() && path.is_absolute() {
            return path.join("axysbrowser");
        }
    }

    if let Some(home) = home {
        let mut path = PathBuf::from(home);

        for part in home_fallback {
            path = path.join(part);
        }

        return path.join("axysbrowser");
    }

    PathBuf::from(last_resort)
}

fn xdg_dir(xdg_var: &str, home_fallback: &[&str], last_resort: &str) -> PathBuf {
    xdg_dir_from(
        env::var_os(xdg_var).as_deref(),
        env::var_os("HOME").as_deref(),
        home_fallback,
        last_resort,
    )
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

    let result: io::Result<()> = (|| {
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

        fs::rename(&temporary, path)?;

        Ok(())
    })();

    // A failed write or rename used to leave `.name.pid.tmp` behind forever,
    // one per failed save (a full disk could fill up with them).
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }

    result?;

    // Best effort: also persist the directory entry (a failed directory
    // fsync must not fail the save on filesystems that don't support it).
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }

    Ok(())
}

/// Moves a profile file that exists but cannot be parsed aside to a unique
/// `<name>.corrupt-<timestamp>-<pid>` sidecar before the loader falls back to
/// defaults. The next save would overwrite the broken file, so keeping a
/// copy is the only way to recover a hand edit or a file written by a newer
/// version. Rename is atomic, and the backup is tightened to owner-only mode
/// because it can contain history, bookmarks or credentials.
pub fn quarantine_corrupt(path: &Path) {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let backup = path.with_file_name(format!("{name}.corrupt-{stamp}-{}", std::process::id()));

    // Tighten the original before the rename: the backup may outlive the
    // process, and profile files are private.
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));

    match fs::rename(path, &backup) {
        Ok(()) => eprintln!(
            "{} could not be read; kept a copy as {}",
            path.display(),
            backup.display()
        ),
        Err(error) => eprintln!("Could not back up unreadable {}: {error}", path.display()),
    }
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

    #[test]
    fn a_failed_rename_does_not_leave_a_temp_file() {
        let dir = unique_dir("failed-rename");
        let _ = fs::remove_dir_all(&dir);

        fs::create_dir_all(&dir).unwrap();

        // A directory where the file belongs: the temp file is written, but
        // the rename fails. Nothing may be left behind.
        let path = dir.join("settings.toml");

        fs::create_dir(&path).unwrap();

        assert!(atomic_write(&path, "data").is_err());

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
    fn an_absolute_xdg_directory_wins_over_home() {
        use std::ffi::OsStr;

        let dir = xdg_dir_from(
            Some(OsStr::new("/tmp/custom-config")),
            Some(OsStr::new("/home/user")),
            &[".config"],
            "fallback",
        );

        assert_eq!(dir, PathBuf::from("/tmp/custom-config/axysbrowser"));
    }

    #[test]
    fn relative_or_empty_xdg_directories_are_ignored() {
        use std::ffi::OsStr;

        for xdg in ["relative/config", "", "."] {
            let dir = xdg_dir_from(
                Some(OsStr::new(xdg)),
                Some(OsStr::new("/home/user")),
                &[".config"],
                "fallback",
            );

            assert_eq!(
                dir,
                PathBuf::from("/home/user/.config/axysbrowser"),
                "XDG dir {xdg:?} must fall back to HOME"
            );
        }
    }

    #[test]
    fn without_home_the_last_resort_is_used() {
        let dir = xdg_dir_from(None, None, &[".config"], "axysbrowser-fallback");

        assert_eq!(dir, PathBuf::from("axysbrowser-fallback"));
    }

    #[test]
    fn a_corrupt_file_is_kept_as_a_private_backup() {
        let dir = unique_dir("quarantine");
        let _ = fs::remove_dir_all(&dir);

        fs::create_dir_all(&dir).unwrap();

        let path = dir.join("bookmarks.toml");

        fs::write(&path, "bookmarks = [oops").unwrap();

        quarantine_corrupt(&path);

        assert!(!path.exists());
        let backup = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("bookmarks.toml.corrupt-")
            })
            .expect("the corrupt file must be preserved");

        assert_eq!(
            fs::read_to_string(backup.path()).unwrap(),
            "bookmarks = [oops"
        );
        assert_eq!(
            fs::metadata(backup.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
