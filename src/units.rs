//! Reconstructing cargo's build units from the contents of `target/`.
//!
//! Cargo scatters each build unit across several places that share one 16-hex
//! metadata hash `H`:
//!
//! * `<profile>/.fingerprint/<pkg-name>-<H>/` — the fingerprint dir,
//! * `<profile>/deps/(lib)?<crate-name>-<H>.*` — the compiled artifacts,
//! * `<profile>/build/<pkg-name>-<H>/` — build-script compile/run output.
//!
//! The fingerprint/build dirs use the **package** name (hyphens); `deps/` uses
//! the **crate** name (underscores). Every *compile* unit also drops a depfile
//! whose source paths pin it to an exact package version. [`collect_units`] ties
//! all of that back together into [`Unit`]s.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

/// Which package identity a unit belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    /// A registry crate, keyed as `"{name}-{version}"` (matches `LiveSet`).
    Registry(String),
    /// A path/git/workspace package, identified by name only (these don't
    /// accumulate stale versions in `target/`; only removal matters).
    Local(String),
    /// No depfile to read a version from (a build-script *run* unit).
    Unknown,
}

/// A rough classification of the unit, mostly to spot build-script run units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Lib,
    Bin,
    BuildScriptCompile,
    BuildScriptRun,
    Other,
}

/// One reconstructed build unit and every file/dir that belongs to it.
#[derive(Debug, Clone)]
pub struct Unit {
    /// Hyphenated package name (as used in the fingerprint/build dir names).
    pub pkg_name: String,
    /// The 16-hex metadata hash shared by this unit's files.
    pub hash: String,
    pub kind: UnitKind,
    pub version: Version,
    /// Files and directories owned exclusively by this unit.
    pub paths: Vec<PathBuf>,
    /// Total bytes across `paths`.
    pub bytes: u64,
}

/// Split a `<name>-<16hex>` directory name into its package name and hash.
pub fn split_name_hash(dir_name: &str) -> Option<(String, String)> {
    let idx = dir_name.rfind('-')?;
    let (name, hash) = (&dir_name[..idx], &dir_name[idx + 1..]);
    if hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some((name.to_string(), hash.to_string()))
    } else {
        None
    }
}

/// Extract a deps-file's owning hash: strip an optional `lib` prefix, then take
/// the 16-hex token after the first `-` (crate names never contain `-`).
fn deps_file_hash(file_name: &str) -> Option<String> {
    let stem = file_name.strip_prefix("lib").unwrap_or(file_name);
    let dash = stem.find('-')?;
    let rest = &stem[dash + 1..];
    let hash = rest.get(..16)?;
    hash.bytes()
        .all(|b| b.is_ascii_hexdigit())
        .then(|| hash.to_string())
}

/// Directories directly under `target/` (depth ≤ 2) that hold a `.fingerprint`
/// dir — i.e. `debug`, `release`, and per-target-triple `<triple>/{debug,release}`.
pub fn profile_roots(target: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for entry in WalkDir::new(target)
        .min_depth(1)
        .max_depth(2)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() && entry.path().join(".fingerprint").is_dir() {
            roots.push(entry.path().to_path_buf());
        }
    }
    roots
}

/// Reconstruct every build unit across every profile root of `target`.
pub fn collect_units(target: &Path) -> Result<Vec<Unit>> {
    let mut units = Vec::new();
    for root in profile_roots(target) {
        collect_profile_units(&root, &mut units)?;
    }
    Ok(units)
}

fn collect_profile_units(root: &Path, units: &mut Vec<Unit>) -> Result<()> {
    // Index every deps/ file by its owning hash so each unit is an O(1) lookup.
    let deps_dir = root.join("deps");
    let mut deps_by_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
    if let Ok(entries) = fs::read_dir(&deps_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(hash) = name.to_str().and_then(deps_file_hash) {
                deps_by_hash.entry(hash).or_default().push(entry.path());
            }
        }
    }

    let fingerprint_dir = root.join(".fingerprint");
    let entries = match fs::read_dir(&fingerprint_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Some((pkg_name, hash)) = entry.file_name().to_str().and_then(split_name_hash) else {
            continue;
        };

        // Gather this unit's paths: fingerprint dir + deps files + build dir.
        let mut paths = vec![entry.path()];
        if let Some(deps) = deps_by_hash.get(&hash) {
            paths.extend(deps.iter().cloned());
        }
        let build_dir = root.join("build").join(format!("{pkg_name}-{hash}"));
        if build_dir.is_dir() {
            paths.push(build_dir.clone());
        }

        let kind = classify(&entry.path());
        let version = resolve_version(&pkg_name, &paths, &build_dir);
        let bytes = measure(&paths);

        units.push(Unit {
            pkg_name,
            hash,
            kind,
            version,
            paths,
            bytes,
        });
    }
    Ok(())
}

/// Classify a unit by the marker files cargo writes in its fingerprint dir.
fn classify(fingerprint_dir: &Path) -> UnitKind {
    let Ok(entries) = fs::read_dir(fingerprint_dir) else {
        return UnitKind::Other;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("run-build-script") {
            return UnitKind::BuildScriptRun;
        }
        if name.starts_with("build-script") || name.starts_with("build_script") {
            return UnitKind::BuildScriptCompile;
        }
        if name.starts_with("lib-") {
            return UnitKind::Lib;
        }
        if name.starts_with("bin-") {
            return UnitKind::Bin;
        }
    }
    UnitKind::Other
}

/// Determine a unit's version by reading any depfile among its paths (a `deps`
/// `*.d`, or a build-script's `build_script_build-*.d`).
fn resolve_version(pkg_name: &str, paths: &[PathBuf], build_dir: &Path) -> Version {
    let mut depfiles: Vec<PathBuf> = paths
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "d"))
        .cloned()
        .collect();
    if let Ok(entries) = fs::read_dir(build_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "d") {
                depfiles.push(path);
            }
        }
    }

    for depfile in &depfiles {
        if let Some(version) = version_from_depfile(depfile, pkg_name) {
            return version;
        }
    }
    Version::Unknown
}

fn version_from_depfile(depfile: &Path, pkg_name: &str) -> Option<Version> {
    let text = fs::read_to_string(depfile).ok()?;
    let mut saw_source = false;
    for token in text.split_whitespace() {
        if let Some(pkgdir) = registry_pkgdir(token, pkg_name) {
            return Some(Version::Registry(pkgdir));
        }
        if token.contains('/') {
            saw_source = true;
        }
    }
    // A depfile that references sources but no registry path is a path/git/
    // workspace crate: identify it by name.
    saw_source.then(|| Version::Local(pkg_name.to_string()))
}

/// If `token` is a source path under a registry checkout for `pkg_name`, return
/// the `"{name}-{version}"` package directory component.
fn registry_pkgdir(token: &str, pkg_name: &str) -> Option<String> {
    const MARKER: &str = "/registry/src/";
    let start = token.find(MARKER)? + MARKER.len();
    let mut parts = token[start..].split('/');
    let _registry_index = parts.next()?;
    let pkgdir = parts.next()?;
    pkgdir
        .starts_with(&format!("{pkg_name}-"))
        .then(|| pkgdir.to_string())
}

/// Total bytes across a set of files/directories.
fn measure(paths: &[PathBuf]) -> u64 {
    let mut bytes = 0u64;
    for path in paths {
        for entry in WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    bytes += meta.len();
                }
            }
        }
    }
    bytes
}
