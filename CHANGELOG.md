# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-09-14

Hardening release following an adversarial security review. The core containment
fence held — no path-traversal or symlink escape out of a real `target/` was
achievable — so these are robustness fixes, all confined to the target tree.

### Security

- **DoS:** deep-tree deletion no longer risks a stack overflow. The recursive
  `std::fs::remove_dir_all` is replaced with a heap-iterative delete, so a
  pathologically deep (attacker-supplied) directory can't crash the sweep (F1).
- **Fail-open:** a parseable-but-empty `Cargo.lock` (zero packages) now fails
  closed instead of treating every artifact as unreachable and deleting them (F2).
- Workspace-member globs are confined to the project tree: a `..` component or a
  path resolving outside the workspace root is rejected, so a crafted
  `Cargo.toml` can't read files elsewhere (F3).
- A metadata-hash collision across two units is detected and both are skipped, so
  a live unit's artifacts can't be dragged into a dead unit's removal (F4).
- Documented that the `CACHEDIR.TAG` target-dir fence is advisory, not
  adversarial, with its confined blast radius (F5).

### Changed

- Add the GitHub CLI (`gh`) to the Nix dev shell.

## [0.1.0] - 2026-09-14

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
- Rust edition 2024; minimum supported Rust version 1.85. The dev shell / Nix
  build use the latest stable toolchain (decoupled from the MSRV).

[Unreleased]: https://github.com/BoatRamp/cargo-sift/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/BoatRamp/cargo-sift/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/BoatRamp/cargo-sift/releases/tag/v0.1.0
