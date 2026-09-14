//! `cargo-sift` — remove build artifacts from Cargo `target/` directories that
//! the current build graph can no longer reference.
//!
//! Where `cargo clean` deletes everything and mtime sweepers delete still-valid
//! artifacts, `cargo sift` performs a deterministic **reachability GC**: it
//! reads `Cargo.lock` and the workspace manifests to learn what the project
//! resolves to, reconstructs the build units sitting in `target/`, and removes
//! only the unreachable ones — dependency versions no longer in the lockfile,
//! and artifacts for removed packages and targets.
//!
//! The pieces: [`discovery`] finds target dirs, [`resolve`] computes the live
//! set from `Cargo.lock` (no cargo subprocess), [`units`] reconstructs units
//! from disk, and [`gc`] classifies and removes them.

pub mod cli;
pub mod discovery;
pub mod gc;
pub mod human;
pub mod resolve;
pub mod units;

use std::path::Path;

use anyhow::{Context, Result};

use cli::Args;

/// Entry point shared by the binary: resolve target directories, then GC each
/// one against its own current resolve.
pub fn run(args: Args) -> Result<()> {
    let targets = discovery::resolve_targets(&args.paths, args.recursive)
        .context("failed to locate Cargo target directories")?;

    if targets.is_empty() {
        let where_ = args
            .paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("cargo-sift: no Cargo target directories found under {where_}");
        return Ok(());
    }

    let mut grand = gc::Stats::default();
    let mut cleaned = 0usize;
    for target in &targets {
        // Fail safe: without a resolve we cannot know what's live, so skip.
        let live = match resolve::resolve(target) {
            Ok(live) => live,
            Err(err) => {
                eprintln!(
                    "cargo-sift: skipping {} — {err}; nothing removed",
                    target.display()
                );
                continue;
            }
        };

        let plan = gc::plan(target, &live)
            .with_context(|| format!("planning GC for {}", target.display()))?;
        let stats = gc::execute(&plan, args.dry_run, args.verbose)?;
        if !args.quiet {
            print_target_result(target, &plan, &stats, args.dry_run);
        }
        grand.merge(&stats);
        cleaned += 1;
    }

    if cleaned > 1 || args.quiet {
        let verb = if args.dry_run { "would free" } else { "freed" };
        println!(
            "cargo-sift: {verb} {} across {cleaned} target director{} ({} units removed)",
            human::format_size(grand.bytes_freed),
            if cleaned == 1 { "y" } else { "ies" },
            grand.units_removed,
        );
    }
    if grand.errors > 0 {
        eprintln!(
            "cargo-sift: {} unit(s) could not be fully removed",
            grand.errors
        );
    }
    Ok(())
}

fn print_target_result(target: &Path, plan: &gc::Plan, stats: &gc::Stats, dry_run: bool) {
    if plan.removals.is_empty() {
        println!(
            "{}: {} — already tidy",
            target.display(),
            human::format_size(plan.dir_bytes)
        );
        return;
    }
    let verb = if dry_run { "would free" } else { "freed" };
    let breakdown = plan
        .tally()
        .into_iter()
        .map(|(reason, count, bytes)| {
            format!("{count} {} {}", reason.label(), human::format_size(bytes))
        })
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "{}: {} → {verb} {} ({} units: {breakdown})",
        target.display(),
        human::format_size(plan.dir_bytes),
        human::format_size(stats.bytes_freed),
        stats.units_removed,
    );
}
