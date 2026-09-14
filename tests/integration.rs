//! End-to-end tests over synthetic `target/` trees. We fabricate the real cargo
//! unit layout (fingerprint dirs, deps artifacts, depfiles, incremental
//! sessions) and drive the GC against a hand-built `LiveSet`, plus a real
//! `resolve` over a fabricated `Cargo.lock` + manifests.

use std::fs;
use std::path::Path;

use cargo_sift::discovery;
use cargo_sift::gc::{self, Reason};
use cargo_sift::resolve::{self, LiveSet};

const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n";
const REGISTRY: &str = "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f";

fn make_target(dir: &Path) -> std::path::PathBuf {
    let debug = dir.join("debug");
    fs::create_dir_all(debug.join(".fingerprint")).unwrap();
    fs::create_dir_all(debug.join("deps")).unwrap();
    fs::write(dir.join("CACHEDIR.TAG"), SIGNATURE).unwrap();
    debug
}

fn crate_name(pkg: &str) -> String {
    pkg.replace('-', "_")
}

/// Create a library compile unit for a registry crate `pkg` at `version`.
fn add_registry_unit(profile: &Path, pkg: &str, version: &str, hash: &str) {
    let cn = crate_name(pkg);
    let fp = profile.join(".fingerprint").join(format!("{pkg}-{hash}"));
    fs::create_dir_all(&fp).unwrap();
    fs::write(fp.join(format!("lib-{cn}")), b"").unwrap();
    let deps = profile.join("deps");
    fs::write(deps.join(format!("lib{cn}-{hash}.rlib")), vec![0u8; 4096]).unwrap();
    fs::write(
        deps.join(format!("{cn}-{hash}.d")),
        format!("lib{cn}-{hash}.rlib: {REGISTRY}/{pkg}-{version}/src/lib.rs\n"),
    )
    .unwrap();
}

/// Create a local compile unit: a `marker` file in the fingerprint dir picks the
/// kind (`lib-…` = library, `test-…` = test target), and the depfile has only a
/// local source path.
fn add_local_unit(profile: &Path, name: &str, hash: &str, marker: &str) {
    let cn = crate_name(name);
    let fp = profile.join(".fingerprint").join(format!("{name}-{hash}"));
    fs::create_dir_all(&fp).unwrap();
    fs::write(fp.join(format!("{marker}-{cn}")), b"").unwrap();
    let deps = profile.join("deps");
    fs::write(deps.join(format!("lib{cn}-{hash}.rlib")), vec![0u8; 2048]).unwrap();
    fs::write(
        deps.join(format!("{cn}-{hash}.d")),
        format!("lib{cn}-{hash}.rlib: src/lib.rs\n"),
    )
    .unwrap();
}

#[test]
fn removes_dead_dependency_version_keeps_live() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let debug = make_target(&target);
    add_registry_unit(&debug, "anyhow", "1.0.104", "aaaaaaaaaaaaaaaa");
    add_registry_unit(&debug, "anyhow", "1.0.86", "bbbbbbbbbbbbbbbb");

    let live = LiveSet::new(["anyhow".into()], ["anyhow-1.0.104".into()]);
    let plan = gc::plan(&target, &live).unwrap();

    assert_eq!(plan.removals.len(), 1);
    assert_eq!(plan.removals[0].reason, Reason::DeadVersion);
    assert_eq!(plan.removals[0].label, "anyhow-1.0.86");

    gc::execute(&plan, false, false).unwrap();
    assert!(!debug.join(".fingerprint/anyhow-bbbbbbbbbbbbbbbb").exists());
    assert!(debug.join(".fingerprint/anyhow-aaaaaaaaaaaaaaaa").exists());
    assert!(!debug.join("deps/libanyhow-bbbbbbbbbbbbbbbb.rlib").exists());
    assert!(debug.join("deps/libanyhow-aaaaaaaaaaaaaaaa.rlib").exists());
    assert!(target.join("CACHEDIR.TAG").exists());
}

#[test]
fn same_version_variants_are_all_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let debug = make_target(&target);
    // Two feature/target variants of one live version — both must survive.
    add_registry_unit(&debug, "hashbrown", "0.15.2", "1111111111111111");
    add_registry_unit(&debug, "hashbrown", "0.15.2", "2222222222222222");

    let live = LiveSet::new(["hashbrown".into()], ["hashbrown-0.15.2".into()]);
    let plan = gc::plan(&target, &live).unwrap();
    assert!(plan.removals.is_empty());
}

#[test]
fn handles_cross_compilation_profile_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let host = make_target(&target); // target/debug
    // Cross-compilation layout: target/<triple>/debug is a second profile root.
    let cross = target.join("x86_64-unknown-linux-musl/debug");
    fs::create_dir_all(cross.join(".fingerprint")).unwrap();
    fs::create_dir_all(cross.join("deps")).unwrap();

    add_registry_unit(&host, "anyhow", "1.0.86", "aaaaaaaaaaaaaaaa");
    add_registry_unit(&cross, "anyhow", "1.0.86", "bbbbbbbbbbbbbbbb");
    add_registry_unit(&cross, "anyhow", "1.0.104", "cccccccccccccccc"); // live, kept

    let live = LiveSet::new(["anyhow".into()], ["anyhow-1.0.104".into()]);
    let plan = gc::plan(&target, &live).unwrap();

    // The dead version is reaped in BOTH roots; the live one in the cross root stays.
    assert_eq!(plan.removals.len(), 2);
    assert!(
        plan.removals
            .iter()
            .all(|r| r.reason == Reason::DeadVersion)
    );
    gc::execute(&plan, false, false).unwrap();
    assert!(!cross.join(".fingerprint/anyhow-bbbbbbbbbbbbbbbb").exists());
    assert!(cross.join(".fingerprint/anyhow-cccccccccccccccc").exists());
}

#[test]
fn removes_removed_package_lib_but_keeps_live_test_target() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let debug = make_target(&target);
    add_local_unit(&debug, "ghost", "cccccccccccccccc", "lib"); // removed member
    add_local_unit(&debug, "integration", "dddddddddddddddd", "test"); // live test target

    // The resolve knows the app package and its `integration` test target.
    let live = LiveSet::new(["my-app".into(), "integration".into()], []);
    let plan = gc::plan(&target, &live).unwrap();

    assert_eq!(plan.removals.len(), 1);
    assert_eq!(plan.removals[0].reason, Reason::RemovedPackage);
    assert_eq!(plan.removals[0].label, "ghost");
}

#[test]
fn removes_removed_non_lib_target() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let debug = make_target(&target);
    add_local_unit(&debug, "old_bench", "eeeeeeeeeeeeeeee", "bench");

    // `old_bench` is no longer a target of any package.
    let live = LiveSet::new(["my-app".into()], []);
    let plan = gc::plan(&target, &live).unwrap();
    assert_eq!(plan.removals.len(), 1);
    assert_eq!(plan.removals[0].reason, Reason::RemovedTarget);
}

#[test]
fn dry_run_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let debug = make_target(&target);
    add_registry_unit(&debug, "anyhow", "1.0.86", "bbbbbbbbbbbbbbbb");

    let live = LiveSet::new(["anyhow".into()], ["anyhow-1.0.104".into()]);
    let plan = gc::plan(&target, &live).unwrap();
    let stats = gc::execute(&plan, true, false).unwrap();

    assert_eq!(stats.units_removed, 1);
    assert!(stats.bytes_freed >= 4096);
    assert!(debug.join(".fingerprint/anyhow-bbbbbbbbbbbbbbbb").exists());
}

#[test]
fn removes_incremental_for_removed_crate_keeps_live() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let debug = make_target(&target);
    fs::create_dir_all(debug.join("incremental/gonecrate-0abc123def456")).unwrap();
    fs::write(
        debug.join("incremental/gonecrate-0abc123def456/data.bin"),
        vec![0u8; 1024],
    )
    .unwrap();
    fs::create_dir_all(debug.join("incremental/my_app-0live99session")).unwrap();
    fs::write(
        debug.join("incremental/my_app-0live99session/data.bin"),
        vec![0u8; 16],
    )
    .unwrap();

    let live = LiveSet::new(["my-app".into()], []);
    let plan = gc::plan(&target, &live).unwrap();

    assert_eq!(plan.removals.len(), 1);
    assert_eq!(plan.removals[0].reason, Reason::RemovedIncremental);
    gc::execute(&plan, false, false).unwrap();
    assert!(!debug.join("incremental/gonecrate-0abc123def456").exists());
    assert!(debug.join("incremental/my_app-0live99session").exists());
}

#[test]
fn build_script_incremental_sessions_are_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let debug = make_target(&target);
    // `build_script_build` is cargo's reserved name and never in the resolve;
    // its incremental sessions must not be treated as removed.
    fs::create_dir_all(debug.join("incremental/build_script_build-0abc123")).unwrap();
    fs::write(
        debug.join("incremental/build_script_build-0abc123/data.bin"),
        vec![0u8; 32],
    )
    .unwrap();

    let live = LiveSet::new(["my-app".into()], []);
    let plan = gc::plan(&target, &live).unwrap();
    assert!(plan.removals.is_empty());
}

#[test]
fn resolve_reads_lockfile_and_reconstructs_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    fs::create_dir_all(proj.join("src")).unwrap();
    fs::create_dir_all(proj.join("tests")).unwrap();
    fs::write(proj.join("src/lib.rs"), b"").unwrap();
    fs::write(proj.join("tests/integration.rs"), b"").unwrap();
    fs::write(
        proj.join("Cargo.toml"),
        b"[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n[dependencies]\nanyhow = \"1\"\n",
    )
    .unwrap();
    fs::write(
        proj.join("Cargo.lock"),
        b"[[package]]\nname = \"anyhow\"\nversion = \"1.0.104\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n[[package]]\nname = \"my-app\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let target = proj.join("target");
    let live = resolve::resolve(&target).unwrap();

    assert!(live.registry_is_live("anyhow-1.0.104"));
    assert!(!live.registry_is_live("anyhow-1.0.86"));
    assert!(live.name_is_live("my-app")); // package + default lib target
    assert!(live.name_is_live("integration")); // reconstructed test target
    assert!(!live.name_is_live("nonexistent"));
}

#[test]
fn resolve_without_lockfile_fails_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("noproj/target");
    assert!(resolve::resolve(&target).is_err());
}

// --- security regressions (F1–F4 from the adversarial review) ---

#[test]
fn f1_deletes_deeply_nested_unit_without_stack_overflow() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let debug = make_target(&target);
    add_registry_unit(&debug, "anyhow", "1.0.86", "bbbbbbbbbbbbbbbb");
    // Nest a tree deep inside the dead unit's fingerprint dir. The iterative
    // deleter walks it on the heap; the old recursive `remove_dir_all` would
    // overflow the stack on a pathological (attacker-supplied) depth.
    let mut deep = debug.join(".fingerprint/anyhow-bbbbbbbbbbbbbbbb");
    for _ in 0..256 {
        deep = deep.join("d");
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("f"), b"x").unwrap();

    let live = LiveSet::new(["anyhow".into()], ["anyhow-1.0.104".into()]);
    let plan = gc::plan(&target, &live).unwrap();
    let stats = gc::execute(&plan, false, false).unwrap();
    assert_eq!(stats.errors, 0);
    assert!(!debug.join(".fingerprint/anyhow-bbbbbbbbbbbbbbbb").exists());
}

#[test]
fn f2_empty_lockfile_fails_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("Cargo.toml"),
        b"[package]\nname=\"p\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    // Valid TOML, zero packages — must NOT resolve to an empty (delete-all) set.
    fs::write(proj.join("Cargo.lock"), b"version = 4\n").unwrap();
    assert!(resolve::resolve(&proj.join("target")).is_err());
}

#[test]
fn f3_workspace_members_cannot_escape_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    // An "outside" crate (with a lib) the workspace must not reach.
    let outside = tmp.path().join("outside");
    fs::create_dir_all(outside.join("src")).unwrap();
    fs::write(outside.join("src/lib.rs"), b"").unwrap();
    fs::write(
        outside.join("Cargo.toml"),
        b"[package]\nname=\"secret\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    // A workspace whose members maliciously point outside the tree.
    let ws = tmp.path().join("ws");
    fs::create_dir_all(&ws).unwrap();
    fs::write(
        ws.join("Cargo.toml"),
        b"[workspace]\nmembers = [\"../outside\"]\n[package]\nname = \"root\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        ws.join("Cargo.lock"),
        b"[[package]]\nname = \"root\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let live = resolve::resolve(&ws.join("target")).unwrap();
    assert!(live.name_is_live("root"));
    // The outside member's Cargo.toml was never read, so "secret" isn't live.
    assert!(!live.name_is_live("secret"));
}

#[test]
fn f4_hash_collision_skips_both_units() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let debug = make_target(&target);
    // A live unit and a forged "dead" unit sharing the same 16-hex hash.
    add_registry_unit(&debug, "serde", "1.0.0", "abcabcabcabcabca");
    let evil = debug.join(".fingerprint/evil-abcabcabcabcabca");
    fs::create_dir_all(&evil).unwrap();
    fs::write(evil.join("lib-evil"), b"").unwrap();

    let live = LiveSet::new(["serde".into()], ["serde-1.0.0".into()]);
    let plan = gc::plan(&target, &live).unwrap();
    // Colliding units are skipped, so the live crate's artifact is never dragged
    // into a removal.
    assert!(plan.removals.is_empty());
    gc::execute(&plan, false, false).unwrap();
    assert!(debug.join("deps/libserde-abcabcabcabcabca.rlib").exists());
}

#[test]
fn detects_target_dir_by_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    assert!(!discovery::is_target_dir(&target));
    make_target(&target);
    assert!(discovery::is_target_dir(&target));
}

#[test]
fn recursive_finds_marked_targets_only() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    make_target(&root.join("a/target"));
    make_target(&root.join("b/nested/target"));
    fs::create_dir_all(root.join("decoy/target/stuff")).unwrap(); // no marker

    let found = discovery::resolve_targets(&[root.to_path_buf()], true).unwrap();
    assert_eq!(found.len(), 2);
}
