//! Command-line surface for `cargo-sift`.

use std::path::PathBuf;

use clap::Parser;

/// `cargo sift` — remove build artifacts from Cargo `target/` directories that
/// the current build graph can no longer reference.
///
/// Unlike `cargo clean` (which deletes everything) or mtime-based sweepers
/// (which delete still-valid artifacts and force rebuilds), `cargo sift` reads
/// `Cargo.lock` (plus the workspace manifests) to learn what the project
/// resolves to, and removes only what's unreachable: dependency versions no
/// longer in the lockfile, and artifacts for removed packages and targets. It's
/// fully deterministic — no file timestamps are consulted — and refuses to
/// delete anything if the resolve can't be computed.
#[derive(Debug, Parser)]
#[command(
    name = "cargo-sift",
    bin_name = "cargo sift",
    version,
    about = "Remove unreachable build artifacts from Cargo target/ directories.",
    long_about = None,
    after_help = "\
EXAMPLES:
  cargo sift                     GC the current project's target/
  cargo sift --dry-run           show what would be removed, delete nothing
  cargo sift -r ~/src            GC every Cargo project found under ~/src"
)]
pub struct Args {
    /// Project directories or `target/` directories to sift (default: `.`).
    #[arg(default_value = ".", value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Recurse into sub-directories, sifting every Cargo project found.
    #[arg(short = 'r', long)]
    pub recursive: bool,

    /// Show what would be removed without deleting anything.
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// List every removed unit and why.
    #[arg(short = 'v', long, conflicts_with = "quiet")]
    pub verbose: bool,

    /// Suppress per-target output; print only the final summary.
    #[arg(short = 'q', long)]
    pub quiet: bool,
}
