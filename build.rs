//! Rebuild when the translations change.
//!
//! `rust_i18n::i18n!` embeds `locales/` at compile time. Cargo does not know
//! that, so editing only a `.yml` file kept serving the old strings (or, after
//! a truncated file, raw keys). Re-running a build script is not enough to
//! recompile the crate: its output has to change. So the script exports a
//! hash of the translations as an environment variable of the crate build,
//! which makes Cargo rebuild exactly when a translation changes.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};

fn main() {
    println!("cargo:rerun-if-changed=locales");

    let mut files: Vec<_> = fs::read_dir("locales")
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();

    files.sort();

    let mut hasher = DefaultHasher::new();

    for path in files {
        path.hash(&mut hasher);
        fs::read(&path).unwrap_or_default().hash(&mut hasher);
    }

    println!("cargo:rustc-env=AXYS_LOCALES_HASH={:016x}", hasher.finish());
}
