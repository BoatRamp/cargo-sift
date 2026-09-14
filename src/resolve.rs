//! The current resolve, computed **without invoking cargo** — parsed from
//! `Cargo.lock` and the workspace manifests.
//!
//! `Cargo.lock` is the authoritative, platform-agnostic superset of every
//! package the build graph can reference, and it's exactly what `target/` was
//! built against. Reading it directly needs no subprocess, always works
//! offline, and never re-resolves to something newer than what's on disk.
//!
//! `Cargo.lock` doesn't list per-package *target* names (lib/bin/test/example),
//! which cargo uses to name units on disk, so we reconstruct those ourselves
//! from each workspace member's `Cargo.toml` plus cargo's target
//! auto-discovery conventions. Reconstruction errs toward *more* names, which is
//! the safe direction: an extra name only ever keeps a unit, never deletes one.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The set of identities the current resolve can reference.
#[derive(Debug, Default, Clone)]
pub struct LiveSet {
    /// Registry packages as `"{name}-{version}"` (matches on-disk dir naming).
    registry_versions: HashSet<String>,
    /// Package names AND reconstructed target crate names, normalized
    /// (`-` → `_`), for matching local units, build scripts and incremental.
    names: HashSet<String>,
}

/// Cargo uses the crate name (hyphens → underscores) in `deps/` and
/// `incremental/`; normalize before comparing names.
pub fn normalize(name: &str) -> String {
    name.replace('-', "_")
}

impl LiveSet {
    /// Build a set directly (used by tests to avoid touching the filesystem).
    pub fn new(
        names: impl IntoIterator<Item = String>,
        registry_versions: impl IntoIterator<Item = String>,
    ) -> Self {
        LiveSet {
            names: names.into_iter().map(|n| normalize(&n)).collect(),
            registry_versions: registry_versions.into_iter().collect(),
        }
    }

    /// Is this `"{name}-{version}"` registry identity in the resolve?
    pub fn registry_is_live(&self, name_version: &str) -> bool {
        self.registry_versions.contains(name_version)
    }

    /// Is this package/target name part of the resolve?
    pub fn name_is_live(&self, name: &str) -> bool {
        self.names.contains(&normalize(name))
    }
}

/// Compute the live set for the project owning `target`.
///
/// Fails (rather than returning an empty set) when there's no `Cargo.lock` to
/// resolve against — callers MUST treat failure as "remove nothing", since an
/// empty resolve would make every artifact look unreachable.
pub fn resolve(target: &Path) -> Result<LiveSet> {
    let root = workspace_root(target).with_context(|| {
        format!(
            "no Cargo.lock found at or above {}; cannot determine the resolve",
            target.display()
        )
    })?;

    let (registry_versions, mut names) = parse_lock(&root.join("Cargo.lock"))
        .with_context(|| format!("reading {}", root.join("Cargo.lock").display()))?;

    for member in members(&root)? {
        if let Ok(targets) = member_targets(&member) {
            names.extend(targets);
        }
    }

    Ok(LiveSet {
        registry_versions,
        names,
    })
}

/// The workspace root is the nearest ancestor holding a `Cargo.lock`.
fn workspace_root(target: &Path) -> Option<PathBuf> {
    let mut dir = target.parent();
    while let Some(current) = dir {
        if current.join("Cargo.lock").is_file() {
            return Some(current.to_path_buf());
        }
        dir = current.parent();
    }
    None
}

/// Parse `Cargo.lock` into `(registry "name-version" set, normalized names)`.
fn parse_lock(path: &Path) -> Result<(HashSet<String>, HashSet<String>)> {
    let text = fs::read_to_string(path)?;
    let value: toml::Value = toml::from_str(&text).context("parsing Cargo.lock")?;

    let mut registry_versions = HashSet::new();
    let mut names = HashSet::new();
    if let Some(packages) = value.get("package").and_then(|p| p.as_array()) {
        for package in packages {
            let name = package
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            names.insert(normalize(name));
            let version = package
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            // `source` is present only for non-local packages; a "registry+…"
            // source is keyed on disk as `name-version`.
            let source = package.get("source").and_then(|s| s.as_str());
            if source.is_some_and(|s| s.starts_with("registry+")) {
                registry_versions.insert(format!("{name}-{version}"));
            }
        }
    }
    Ok((registry_versions, names))
}

/// Directories of the workspace members: the root package (if any) plus every
/// `[workspace].members` entry (glob-expanded).
fn members(root: &Path) -> Result<Vec<PathBuf>> {
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)
        .context("parsing workspace Cargo.toml")?;

    let mut dirs = Vec::new();
    if manifest.get("package").is_some() {
        dirs.push(root.to_path_buf());
    }
    if let Some(patterns) = manifest
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    {
        for pattern in patterns.iter().filter_map(|p| p.as_str()) {
            dirs.extend(expand_member_glob(root, pattern));
        }
    }
    if dirs.is_empty() {
        dirs.push(root.to_path_buf());
    }
    dirs.sort();
    dirs.dedup();
    Ok(dirs)
}

/// Expand a `[workspace].members` pattern (supporting `*` as a path component)
/// into member directories that actually hold a `Cargo.toml`.
fn expand_member_glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut current = vec![root.to_path_buf()];
    for component in pattern.split('/').filter(|c| !c.is_empty()) {
        let mut next = Vec::new();
        for base in &current {
            if component == "*" {
                if let Ok(entries) = fs::read_dir(base) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            next.push(entry.path());
                        }
                    }
                }
            } else {
                let candidate = base.join(component);
                if candidate.is_dir() {
                    next.push(candidate);
                }
            }
        }
        current = next;
    }
    current
        .into_iter()
        .filter(|d| d.join("Cargo.toml").is_file())
        .collect()
}

/// Reconstruct the crate names of every target of the member at `dir`, applying
/// cargo's auto-discovery conventions plus any explicit target tables.
fn member_targets(dir: &Path) -> Result<Vec<String>> {
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(dir.join("Cargo.toml"))?)
        .context("parsing member Cargo.toml")?;
    let package_name = manifest
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str());

    let mut names = Vec::new();

    // Library: `[lib].name`, else the package name if `src/lib.rs` exists.
    if let Some(lib) = manifest.get("lib") {
        let lib_name = lib
            .get("name")
            .and_then(|n| n.as_str())
            .map(String::from)
            .or_else(|| package_name.map(String::from));
        names.extend(lib_name);
    } else if dir.join("src/lib.rs").is_file() {
        names.extend(package_name.map(String::from));
    }

    // Default binary: `src/main.rs` → the package name.
    if dir.join("src/main.rs").is_file() {
        names.extend(package_name.map(String::from));
    }

    // Explicit target tables.
    for section in ["bin", "example", "test", "bench"] {
        if let Some(targets) = manifest.get(section).and_then(|s| s.as_array()) {
            for target in targets {
                if let Some(name) = target.get("name").and_then(|n| n.as_str()) {
                    names.push(name.to_string());
                }
            }
        }
    }

    // Auto-discovered targets (unless disabled).
    let auto = |key: &str| {
        manifest
            .get("package")
            .and_then(|p| p.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    };
    if auto("autobins") {
        collect_dir_targets(&dir.join("src/bin"), &mut names);
    }
    if auto("autoexamples") {
        collect_dir_targets(&dir.join("examples"), &mut names);
    }
    if auto("autotests") {
        collect_dir_targets(&dir.join("tests"), &mut names);
    }
    if auto("autobenches") {
        collect_dir_targets(&dir.join("benches"), &mut names);
    }

    Ok(names.into_iter().map(|n| normalize(&n)).collect())
}

/// Collect target names from a conventional target directory: each `*.rs` file
/// (by stem) and each subdirectory containing a `main.rs` (by directory name).
fn collect_dir_targets(dir: &Path, names: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        } else if path.is_dir() && path.join("main.rs").is_file() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                names.push(name.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_names() {
        let live = LiveSet::new(["aho-corasick".into()], []);
        assert!(live.name_is_live("aho_corasick"));
        assert!(live.name_is_live("aho-corasick"));
    }
}
