# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - Unreleased

Initial release.

### Added

- Deterministic **reachability GC** for Cargo `target/` directories: removes only
  build artifacts the current build graph can no longer reference.
- Resolve computed by parsing `Cargo.lock` and the workspace manifests — no
  `cargo` subprocess, always works offline. Per-package target names are
  reconstructed from each member's `Cargo.toml` and cargo's auto-discovery rules.
- Removal categories: dead dependency versions, removed packages, removed
  targets, orphan build scripts, and removed incremental sessions.
- Cross-compilation support: host and per-target (`target/<triple>/`) profile
  roots are both swept; platform-specific dependencies are correctly kept.
- Safety: a target directory is only touched if it carries cargo's `CACHEDIR.TAG`
  / `.rustc_info.json` marker; the run fails closed (deletes nothing) when the
  resolve can't be computed; emptied directories are pruned but the `target/`
  root is never removed.
- CLI flags: `--recursive`, `--dry-run`, `--verbose`, `--quiet`.

[Unreleased]: https://github.com/BoatRamp/cargo-sift/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/BoatRamp/cargo-sift/releases/tag/v0.1.0
