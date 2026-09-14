//! Locating *real* Cargo target directories.
//!
//! We never delete inside a directory merely because it is named `target`. A
//! directory only counts as a Cargo target dir if it carries a marker Cargo
//! itself writes: a `CACHEDIR.TAG` with the standard cache signature, or a
//! `.rustc_info.json`. This is the safety fence that keeps `cargo sift` from
//! wandering into an unrelated `target/` folder.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// The standard `CACHEDIR.TAG` signature Cargo writes at the root of `target/`.
const CACHEDIR_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// Is `dir` a Cargo-managed target directory?
pub fn is_target_dir(dir: &Path) -> bool {
    has_cachedir_tag(dir) || dir.join(".rustc_info.json").is_file()
}

fn has_cachedir_tag(dir: &Path) -> bool {
    match std::fs::read(dir.join("CACHEDIR.TAG")) {
        Ok(bytes) => bytes.starts_with(CACHEDIR_SIGNATURE),
        Err(_) => false,
    }
}

/// Resolve the user-supplied paths into a de-duplicated list of target
/// directories. In `recursive` mode every marked target under each path is
/// found; otherwise each path is resolved to a single target dir.
pub fn resolve_targets(paths: &[PathBuf], recursive: bool) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        if recursive {
            for target in find_targets_recursive(path) {
                add_unique(&mut out, target);
            }
        } else if let Some(target) = target_dir_for(path) {
            add_unique(&mut out, target);
        }
    }
    Ok(out)
}

fn add_unique(out: &mut Vec<PathBuf>, path: PathBuf) {
    let canonical = std::fs::canonicalize(&path).unwrap_or(path);
    if !out.contains(&canonical) {
        out.push(canonical);
    }
}

/// Map a single path to its target directory: the path itself if it is one,
/// otherwise the project's target dir (`$CARGO_TARGET_DIR` or `<path>/target`).
fn target_dir_for(path: &Path) -> Option<PathBuf> {
    if is_target_dir(path) {
        return Some(path.to_path_buf());
    }
    let candidate = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => path.join("target"),
    };
    is_target_dir(&candidate).then_some(candidate)
}

/// Walk `root`, collecting every marked target directory. Descent stops at each
/// target dir (they never nest) and skips `.git` trees.
fn find_targets_recursive(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if is_target_dir(&dir) {
            found.push(dir);
            continue; // a target dir does not contain another target dir
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            // `file_type()` does not follow symlinks, so symlinked dirs report
            // `is_dir() == false` and are naturally skipped.
            if file_type.is_dir() && entry.file_name() != ".git" {
                stack.push(entry.path());
            }
        }
    }
    found
}
