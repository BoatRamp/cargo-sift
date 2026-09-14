# Security Policy

## Supported versions

`cargo-sift` is pre-1.0; security fixes are released against the latest published
version.

## Reporting a vulnerability

`cargo-sift` deletes files, so the security-relevant failure mode is **deleting
something it shouldn't** — a case where an artifact the current build graph can
still reach is removed, or where a directory outside a real Cargo `target/` is
touched.

If you find such a case, or any other vulnerability, please report it privately:

- Use GitHub's [private vulnerability reporting](https://github.com/BoatRamp/cargo-sift/security/advisories/new), or
- Email **giacomo.cariello@uranion.ai**.

Please include steps to reproduce (ideally the smallest `Cargo.toml` /
`Cargo.lock` and `target/` layout that triggers it). We aim to acknowledge
reports within a few days.

Please do **not** open a public issue for a vulnerability until it has been
addressed.

## Scope note

`cargo sift` only ever removes files inside directories it has positively
identified as Cargo target directories (via the `CACHEDIR.TAG` / `.rustc_info.json`
marker), and it fails closed — deleting nothing — when it cannot compute the
project's resolve. A run always supports `--dry-run` to preview exactly what
would be removed.
