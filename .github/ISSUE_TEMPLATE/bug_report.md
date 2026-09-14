---
name: Bug report
about: Something cargo-sift did wrong (especially: removed something it shouldn't)
title: ""
labels: bug
assignees: ""
---

**What happened**
A clear description of the bug. If `cargo sift` removed an artifact that was
still needed (a rebuild happened afterward), please say so explicitly.

**To reproduce**
Ideally the smallest project that triggers it. Helpful details:

- Output of `cargo sift --dry-run --verbose` on the affected project
- The relevant part of `Cargo.lock` / `Cargo.toml`
- The `target/` layout (e.g. `ls target/debug/.fingerprint`)

**Expected behavior**
What you expected to happen.

**Environment**

- `cargo sift --version`:
- `cargo --version` / `rustc --version`:
- OS:

**Additional context**
Anything else relevant (workspace? cross-compilation? unusual target dir?).
