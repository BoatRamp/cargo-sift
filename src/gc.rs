//! The garbage collector: classify every unit in `target/` against the current
//! resolve, then remove what's unreachable. Fully deterministic — no file
//! timestamps are consulted.
//!
//! A unit is removed when it is:
//! * a **dead version** — a registry crate whose `name-version` isn't in
//!   `Cargo.lock` (superseded by a bump, or a dependency removed entirely),
//! * a **removed package** — a workspace/path crate's library that's gone,
//! * a **removed target** — a bin/test/example/bench whose target no longer
//!   exists in its package,
//! * an **orphan build script** — a build-script run unit whose package is gone,
//! * a **removed incremental** — an incremental session for a crate that's gone.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

use crate::resolve::LiveSet;
use crate::units::{self, UnitKind, Version};

/// Why a unit is being removed (for reporting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    DeadVersion,
    RemovedPackage,
    RemovedTarget,
    OrphanBuildScript,
    RemovedIncremental,
}

impl Reason {
    pub fn label(self) -> &'static str {
        match self {
            Reason::DeadVersion => "dead version",
            Reason::RemovedPackage => "removed package",
            Reason::RemovedTarget => "removed target",
            Reason::OrphanBuildScript => "orphan build script",
            Reason::RemovedIncremental => "removed incremental",
        }
    }
}

/// One thing to remove: all files/dirs of a unit (or an incremental session).
#[derive(Debug, Clone)]
pub struct Removal {
    pub label: String,
    pub reason: Reason,
    pub paths: Vec<PathBuf>,
    pub bytes: u64,
}

/// The removal plan for one target directory.
#[derive(Debug)]
pub struct Plan {
    pub target: PathBuf,
    pub removals: Vec<Removal>,
    pub remove_bytes: u64,
    pub dir_bytes: u64,
}

impl Plan {
    /// Removal counts and bytes grouped by reason, for a summary line.
    pub fn tally(&self) -> Vec<(Reason, usize, u64)> {
        let order = [
            Reason::DeadVersion,
            Reason::RemovedPackage,
            Reason::RemovedTarget,
            Reason::OrphanBuildScript,
            Reason::RemovedIncremental,
        ];
        order
            .into_iter()
            .filter_map(|reason| {
                let matching: Vec<&Removal> = self
                    .removals
                    .iter()
                    .filter(|r| r.reason == reason)
                    .collect();
                if matching.is_empty() {
                    return None;
                }
                Some((
                    reason,
                    matching.len(),
                    matching.iter().map(|r| r.bytes).sum(),
                ))
            })
            .collect()
    }
}

/// Running totals across one or more targets.
#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub units_removed: u64,
    pub bytes_freed: u64,
    pub errors: u64,
}

impl Stats {
    pub fn merge(&mut self, other: &Stats) {
        self.units_removed += other.units_removed;
        self.bytes_freed += other.bytes_freed;
        self.errors += other.errors;
    }
}

/// Compute what to remove from `target` given the current resolve.
pub fn plan(target: &Path, live: &LiveSet) -> Result<Plan> {
    let units = units::collect_units(target)?;
    let dir_bytes = dir_size(target);
    let mut removals = Vec::new();

    for unit in &units {
        let removal = match &unit.version {
            Version::Registry(name_version) => (!live.registry_is_live(name_version))
                .then(|| removal(unit, name_version.clone(), Reason::DeadVersion)),
            Version::Local(name) => (!live.name_is_live(name)).then(|| {
                // A library's fingerprint dir is named after the package; other
                // targets after the target itself.
                let reason = if unit.kind == UnitKind::Lib {
                    Reason::RemovedPackage
                } else {
                    Reason::RemovedTarget
                };
                removal(unit, unit.pkg_name.clone(), reason)
            }),
            Version::Unknown => (!live.name_is_live(&unit.pkg_name)).then(|| {
                removal(
                    unit,
                    format!("{} (build script)", unit.pkg_name),
                    Reason::OrphanBuildScript,
                )
            }),
        };
        removals.extend(removal);
    }

    plan_incremental(target, live, &mut removals);

    let remove_bytes = removals.iter().map(|r| r.bytes).sum();
    Ok(Plan {
        target: target.to_path_buf(),
        removals,
        remove_bytes,
        dir_bytes,
    })
}

fn removal(unit: &units::Unit, label: String, reason: Reason) -> Removal {
    Removal {
        label,
        reason,
        paths: unit.paths.clone(),
        bytes: unit.bytes,
    }
}

/// Incremental sessions live at `<profile>/incremental/<crate>-<session>/`. The
/// crate name is the part before the single `-` (crate names never contain one).
/// A session whose crate is gone from the resolve is removed.
fn plan_incremental(target: &Path, live: &LiveSet, removals: &mut Vec<Removal>) {
    for root in units::profile_roots(target) {
        let entries = match fs::read_dir(root.join("incremental")) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name();
            let Some((crate_name, _session)) = name.to_str().and_then(|n| n.split_once('-')) else {
                continue;
            };
            // `build_script_build` is cargo's reserved crate name for *every*
            // build script — it's never in the resolve and can't be attributed
            // to a package, so its sessions must be left alone.
            if crate_name == "build_script_build" {
                continue;
            }
            if !live.name_is_live(crate_name) {
                let path = entry.path();
                let bytes = dir_size(&path);
                removals.push(Removal {
                    label: format!("{crate_name} (incremental)"),
                    reason: Reason::RemovedIncremental,
                    paths: vec![path],
                    bytes,
                });
            }
        }
    }
}

/// Carry out (or, in `dry_run`, simulate) a [`Plan`], pruning dirs left empty.
pub fn execute(plan: &Plan, dry_run: bool, verbose: bool) -> Result<Stats> {
    let mut stats = Stats::default();
    for removal in &plan.removals {
        if verbose {
            let verb = if dry_run { "would remove" } else { "removing" };
            println!(
                "{verb} {} [{}] — {}",
                removal.label,
                removal.reason.label(),
                crate::human::format_size(removal.bytes)
            );
        }
        let mut ok = true;
        if !dry_run {
            for path in &removal.paths {
                if let Err(err) = remove_path(path) {
                    eprintln!("cargo-sift: failed to remove {}: {err}", path.display());
                    ok = false;
                }
                prune_empty_ancestors(path, &plan.target);
            }
        }
        if ok {
            stats.units_removed += 1;
            stats.bytes_freed += removal.bytes;
        } else {
            stats.errors += 1;
        }
    }
    Ok(stats)
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Walk up from a just-removed path, deleting directories our removal left
/// empty (e.g. an emptied `deps/`), stopping at the first non-empty dir or the
/// target root. `remove_dir` only succeeds on empty dirs, so this never touches
/// a directory holding live artifacts or a pre-existing empty dir elsewhere.
fn prune_empty_ancestors(removed: &Path, target: &Path) {
    let mut dir = removed.parent();
    while let Some(current) = dir {
        if current == target || fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

/// Total size in bytes of all files under `path`.
pub fn dir_size(path: &Path) -> u64 {
    let mut bytes = 0u64;
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
    bytes
}
