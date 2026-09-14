# Contributing to cargo-sift

Thanks for your interest in improving `cargo-sift`! This project is small and
contributions of all sizes — bug reports, docs, tests, features — are welcome.

By participating you agree to abide by our
[Code of Conduct](CODE_OF_CONDUCT.md).

## Getting started

The project uses a Nix flake (flake-parts + [crane](https://crane.dev) +
rust-overlay) and [`just`](https://github.com/casey/just). The dev shell pins the
Rust toolchain, so you don't need anything installed globally:

```sh
nix develop           # enter the dev shell (direnv users: `direnv allow`)
```

Prefer plain Cargo? Any toolchain at or above the MSRV (Rust 1.85) works.

## The workflow

```sh
just build      # cargo build
just test       # cargo test
just lint       # cargo clippy --all-targets -- -D warnings
just fmt        # cargo fmt
just ci         # fmt-check + lint + test — the local pre-push gate
just nix-check  # nix flake check — exactly what CI runs
just dogfood    # run cargo-sift on its own target/ (dry-run)
```

Before opening a pull request, please make sure **`just ci`** passes. CI runs the
same checks (fmt, clippy with warnings-as-errors, and tests) on Linux and macOS,
plus a `nix flake check` and an MSRV build.

## How the code is organized

- `src/resolve.rs` — computes the live set from `Cargo.lock` + workspace
  manifests (no `cargo` subprocess).
- `src/units.rs` — reconstructs cargo build units from the contents of `target/`.
- `src/gc.rs` — classifies units against the live set and removes the unreachable.
- `src/discovery.rs` — finds real Cargo `target/` directories.
- `src/cli.rs` / `src/main.rs` — the command-line surface.

The invariant to preserve: **`cargo sift` must never delete an artifact the
current build graph can still reach.** When in doubt, keep the artifact — a
missed cleanup is harmless, a false delete forces a rebuild. New behavior should
come with a test in `tests/integration.rs` that fabricates the relevant
`target/` layout.

## Pull requests

- Keep changes focused; one logical change per PR.
- Add or update tests for behavior changes.
- Update `CHANGELOG.md` under the `[Unreleased]` heading.
- Match the surrounding code style; `cargo fmt` and `clippy` must be clean.

## Reporting bugs

Open an issue with the `cargo sift --version`, your OS, and ideally the output of
`cargo sift --dry-run --verbose` on the affected project. See
[SECURITY.md](SECURITY.md) for anything security-sensitive.

## License

Unless you state otherwise, your contributions are dual-licensed under
[MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE), matching the project.
