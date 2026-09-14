# cargo-sift

[![CI](https://github.com/BoatRamp/cargo-sift/actions/workflows/ci.yml/badge.svg)](https://github.com/BoatRamp/cargo-sift/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cargo-sift.svg)](https://crates.io/crates/cargo-sift)
[![docs.rs](https://img.shields.io/docsrs/cargo-sift)](https://docs.rs/cargo-sift)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue)](#minimum-supported-rust-version)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/cargo-sift.svg)](#license)

> **Reachability GC for Cargo `target/`** — remove what the build graph can no
> longer reach, and nothing it can. Deterministic, offline, no `cargo`
> subprocess.

`cargo sift` reads `Cargo.lock` (plus the workspace manifests) to learn what your
project resolves to, reconstructs the build units sitting in `target/`, and
deletes only the ones that are **unreachable**: dependency versions no longer in
the lockfile, and artifacts for removed packages and targets.

Unlike `cargo clean` (delete everything, rebuild the world) and unlike
mtime-based sweepers (delete still-valid artifacts, forcing needless rebuilds),
`cargo sift` is surgical and **fully deterministic — no file timestamps are ever
consulted, and removing dead artifacts never triggers a rebuild of anything
current.**

```console
$ cargo sift
/home/me/proj/target: 5.02 GiB → freed 3.37 MiB (2 units: 2 dead version 3.37 MiB)
$ cargo build
    Finished `dev` profile in 0.03s      # no-op: nothing live was touched
```

## Contents

- [Why](#why) · [How it works](#how-it-works) · [Safety](#safety)
- [Install](#install) · [Usage](#usage) · [How it compares](#how-it-compares)
- [Contributing](#contributing) · [License](#license)

## Why

A Cargo `target/` only grows. Bump a dependency and the old `.rlib`, its build
script output, and its fingerprints are stranded forever. Over months of churn
that's gigabytes cargo can never use again — but `cargo clean` deletes the *good*
artifacts too, and time-based tools like `cargo-sweep` can't tell a stale
artifact from a valid one that simply hasn't changed, so they delete both and
force a rebuild.

`cargo sift` removes exactly the unreachable set, so what remains is precisely
what your next build would keep.

## How it works

1. **Resolve — from `Cargo.lock`, not `cargo metadata`.** `Cargo.lock` is the
   authoritative, platform-agnostic superset of every package the build graph
   can reference, and it's exactly what `target/` was built against. Reading it
   needs no subprocess, always works offline, and never re-resolves to something
   newer than what's on disk. Per-package *target* names (lib/bin/test/example),
   which cargo uses to name units, aren't in `Cargo.lock`, so `cargo sift`
   reconstructs them from each workspace member's `Cargo.toml` plus cargo's
   target auto-discovery conventions.
2. **Reconstruct.** Every build unit in `target/` shares one 16-hex hash across
   its `.fingerprint/<pkg>-<H>/` dir, its `deps/…-<H>.*` artifacts, and its
   `build/<pkg>-<H>/` dir. Each unit's depfile pins it to an exact version.
3. **Collect the unreachable.** A unit is removed when it is a:
   - **dead version** — a registry crate whose `name-version` isn't in
     `Cargo.lock` (a dependency bumped to a newer version, or removed entirely);
   - **removed package** — a workspace/path crate's library that's gone;
   - **removed target** — a bin/test/example/bench whose target no longer exists;
   - **orphan build script** — a build-script run unit whose package is gone;
   - **removed incremental** — a compilation session for a crate that's gone.

Cross-compilation is handled: host (`target/<profile>/`) and per-target
(`target/<triple>/<profile>/`) roots are both swept, and because `Cargo.lock`
lists every platform's dependencies, cross-target artifacts are correctly kept.

### Safety

- **Fully deterministic.** Liveness comes only from `Cargo.lock` and the
  manifests — never from file ages — so results are reproducible and a valid,
  unchanged artifact is never mistaken for stale.
- **Fail closed.** If there's no `Cargo.lock` to resolve against, that target is
  skipped and nothing is deleted.
- **Target fence.** A directory is only touched if it carries a marker cargo
  writes (`CACHEDIR.TAG` or `.rustc_info.json`), never merely because it's named
  `target`.
- **No needless rebuilds.** Only units the current graph cannot reference are
  removed, so a `cargo build` after `cargo sift` recompiles nothing.

Preview any run with `--dry-run` (add `-v` to list every unit and why).

### What it deliberately keeps

- **Same-version variants.** Distinct hashes of one `name-version` are different
  *configs* (features/target/profile), all potentially live — cargo makes a new
  hash only when the config changes, never a "newer build of the same thing" — so
  they're never touched.
- **Build-script `OUT_DIR`s** of a package that still has a live version, and
  non-lib targets of a *removed* package: kept, since they can't be attributed
  unambiguously from the lockfile alone.

## Install

```sh
# From crates.io
cargo install cargo-sift

# With Nix (flake)
nix profile install github:BoatRamp/cargo-sift

# From source
git clone https://github.com/BoatRamp/cargo-sift
cargo install --path cargo-sift
```

Pre-built binaries for Linux, macOS and Windows are attached to each
[GitHub release](https://github.com/BoatRamp/cargo-sift/releases).

## Usage

`cargo sift` is a Cargo subcommand — run it as `cargo sift`.

```sh
cargo sift              # GC the current project's target/
cargo sift --dry-run    # show what would be removed, delete nothing
cargo sift -v           # list every removed unit and why
cargo sift -r ~/src     # GC every Cargo project found under ~/src
```

| Flag | Description |
|------|-------------|
| `-r, --recursive` | Recurse into sub-directories, sifting every Cargo project found. |
| `-n, --dry-run` | Show what would be removed without deleting anything. |
| `-v, --verbose` | List every removed unit and why. |
| `-q, --quiet` | Print only the final summary. |

Paths default to `.`; each may be a project directory or a `target/` directory.

## How it compares

| Tool | Removes | Forces a rebuild? |
|------|---------|-------------------|
| `cargo clean` | all of `target/` | Always — full rebuild. |
| `cargo-sweep` (mtime) | artifacts older than N days | Often — can't tell stale from valid. |
| **`cargo sift`** | **only unreachable artifacts** | **No.** |
| `cargo-clean-all` / `kondo` | whole `target/` dirs across a tree | Always. |
| `cargo-cache` | the `~/.cargo` registry cache | N/A — not `target/`. |

`cargo sift` occupies the niche once held by the now-unmaintained `cargo-sweep`,
but decides staleness by cargo reachability instead of file age.

## Minimum Supported Rust Version

`cargo-sift` builds on **Rust 1.85** and newer. The MSRV is declared as
`rust-version` in `Cargo.toml` and checked in CI; raising it is a
minor-version-bump change.

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for the dev
workflow (`just ci` mirrors the CI gate) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
The change history lives in [CHANGELOG.md](CHANGELOG.md); to report a
vulnerability, see [SECURITY.md](SECURITY.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
