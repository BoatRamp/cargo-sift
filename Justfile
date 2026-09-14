# cargo-sift — development recipes. Run `just` (or `just --list`) to see them all.
# The devShell (nix develop / direnv) provides `just`, the pinned Rust toolchain,
# and nix.

# The crate / binary name, reused across recipes.
bin := "cargo-sift"

# List the available recipes (default when you run bare `just`).
_default:
    @just --list

# --- Build ---

# Debug build.
build:
    cargo build

# Optimized release build (target/release/{{bin}}).
release:
    cargo build --release

# Type-check all targets without producing binaries (fast feedback).
check:
    cargo check --all-targets

# Remove build artifacts (the blunt instrument this tool exists to avoid).
clean:
    cargo clean

# --- Run ---

# Run the CLI; args pass through, e.g. `just run --time 30 --dry-run ~/src`.
run *ARGS:
    cargo run --quiet -- {{ARGS}}

# Dogfood: reachability-GC this project's own target/ (dry-run).
dogfood: build
    cargo run --quiet -- --dry-run .

# --- Quality ---

# Clippy across all targets, warnings as errors (keeps the tree at zero warnings).
lint:
    cargo clippy --all-targets -- -D warnings

# Format the whole tree in place.
fmt:
    cargo fmt

# Check formatting without writing (for a pre-push gate).
fmt-check:
    cargo fmt --check

# Run the test suite; args pass through, e.g. `just test max_size`.
test *ARGS:
    cargo test {{ARGS}}

# Build the API docs.
doc:
    cargo doc --no-deps

# Fast local pre-push gate: formatting, lint, and tests.
ci: fmt-check lint test

# --- Nix ---

# Build the package with the crane flake (produces ./result).
nix-build:
    nix build .#{{bin}} -L

# Run the flake-built binary; args pass through.
nix-run *ARGS:
    nix run . -- {{ARGS}}

# Exactly what CI runs: build + fmt + clippy + tests through the flake.
nix-check:
    nix flake check -L

# --- Release ---

# Dry-run the crates.io publish (packaging + verify build), no upload.
publish-dry:
    cargo publish --dry-run

# Publish cargo-sift to crates.io. Runs ONLY from an exact `vX.Y.Z` tag matching
# the crate version (the release discipline), and is resumable — an already-
# published version is skipped, so a rerun after a crates.io rate-limit stall is
# safe. Authenticate first: run `cargo login` locally, or set CARGO_REGISTRY_TOKEN
# (how the release CI job runs it).
publish:
    #!/usr/bin/env bash
    set -euo pipefail
    tag="$(git describe --exact-match --tags 2>/dev/null || true)"
    if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
      echo "refusing to publish: HEAD is not on a vX.Y.Z tag (got '${tag:-none}')." >&2
      echo "tag the release first, e.g.: git tag -a vX.Y.Z -m 'cargo-sift X.Y.Z' && git checkout vX.Y.Z" >&2
      exit 1
    fi
    ver="${tag#v}"
    crate_ver="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
    if [[ "$ver" != "$crate_ver" ]]; then
      echo "refusing to publish: tag $tag does not match the crate version $crate_ver." >&2
      exit 1
    fi
    ua="cargo-sift-publish (giacomo.cariello@uranion.ai)"
    code="$(curl -s -A "$ua" -o /dev/null -w '%{http_code}' "https://crates.io/api/v1/crates/cargo-sift/$ver" || echo 000)"
    if [[ "$code" == "200" ]]; then
      echo "== skip cargo-sift@$ver (already on crates.io) =="
      exit 0
    fi
    echo "== publish cargo-sift@$ver =="
    cargo publish
    echo "published cargo-sift $ver"
