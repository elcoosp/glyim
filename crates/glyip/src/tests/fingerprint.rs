//! Tests for fingerprint-based change detection — S12-T02.

use crate::fingerprint::{Fingerprint, FingerprintStore};
use std::fs;
use tempfile::TempDir;

#[test]
fn fingerprint_detects_file_change() {
    // S12-T02: Fingerprint detects file change.
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.g");

    fs::write(&file_path, "fn main() {}\n").unwrap();
    let fp_before = Fingerprint::from_file(&file_path).unwrap();

    fs::write(&file_path, "fn main() { 1 + 1 }\n").unwrap();
    let fp_after = Fingerprint::from_file(&file_path).unwrap();

    assert_ne!(
        fp_before.hash, fp_after.hash,
        "hashes should differ after file change"
    );
    assert!(
        !fp_before.matches(&fp_after),
        "matches() should return false after change"
    );
}

#[test]
fn fingerprint_unchanged_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("stable.g");

    fs::write(&file_path, "fn stable() {}\n").unwrap();
    let fp1 = Fingerprint::from_file(&file_path).unwrap();
    let fp2 = Fingerprint::from_file(&file_path).unwrap();

    assert!(
        fp1.matches(&fp2),
        "identical files should have matching fingerprints"
    );
    assert_eq!(
        fp1.hash, fp2.hash,
        "hashes should be equal for identical content"
    );
}

#[test]
fn fingerprint_from_content() {
    let content = b"fn computed() {}\n";
    let fp = Fingerprint::from_content(content);

    assert!(!fp.hash.is_empty(), "hash should not be empty");
    assert_eq!(
        fp.size,
        content.len() as u64,
        "size should match content length"
    );
    assert_eq!(fp.mtime, 0, "mtime should be 0 for in-memory fingerprint");

    let fp2 = Fingerprint::from_content(content);
    assert!(
        fp.matches(&fp2),
        "same content should produce matching fingerprints"
    );
}

#[test]
fn fingerprint_store_has_changed_detects_new_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("new.g");
    fs::write(&file_path, "fn new() {}\n").unwrap();

    let store = FingerprintStore::new();
    let changed = store.has_changed(&file_path).unwrap();
    assert!(
        changed,
        "new file should be detected as changed (no stored fingerprint)"
    );
}

#[test]
fn fingerprint_store_has_changed_detects_modification() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("mod.g");
    fs::write(&file_path, "fn v1() {}\n").unwrap();

    let mut store = FingerprintStore::new();
    store.update(&file_path).unwrap();

    let changed = store.has_changed(&file_path).unwrap();
    assert!(
        !changed,
        "file should not be detected as changed immediately after update"
    );

    fs::write(&file_path, "fn v2() {}\n").unwrap();
    let changed = store.has_changed(&file_path).unwrap();
    assert!(changed, "modified file should be detected as changed");
}

#[test]
fn fingerprint_store_save_and_load() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("persist.g");
    fs::write(&file_path, "fn persist() {}\n").unwrap();

    let mut store = FingerprintStore::new();
    store.update(&file_path).unwrap();
    assert_eq!(store.len(), 1);

    let target_dir = dir.path().join("target");
    fs::create_dir_all(&target_dir).unwrap();
    store.save_to_dir(&target_dir).unwrap();

    let loaded = FingerprintStore::load_from_dir(&target_dir).unwrap();
    assert_eq!(loaded.len(), 1, "loaded store should have 1 fingerprint");

    let changed = loaded.has_changed(&file_path).unwrap();
    assert!(!changed, "loaded fingerprints should match current file");
}

#[test]
fn has_any_changed_detects_changes() {
    let dir = TempDir::new().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_a = src_dir.join("a.g");
    let file_b = src_dir.join("b.g");
    fs::write(&file_a, "fn a() {}\n").unwrap();
    fs::write(&file_b, "fn b() {}\n").unwrap();

    let mut store = FingerprintStore::new();
    store.update_all(&src_dir, "g").unwrap();
    assert!(!store.has_any_changed(&src_dir, "g", "").unwrap());
    // Edit the file.
    fs::write(&file_a, "fn a_modified() {}\n").unwrap();
    assert!(store.has_any_changed(&src_dir, "g", "").unwrap());
}

#[test]
fn has_any_changed_empty_dir() {
    let dir = TempDir::new().unwrap();
    let empty_src = dir.path().join("src");
    let store = FingerprintStore::new();
    assert!(!store.has_any_changed(&empty_src, "g", "").unwrap());
}

#[test]
fn fingerprint_store_is_empty() {
    let store = FingerprintStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}

// Plan §23.3: a manifest / build-script change must be detected by
// `has_any_changed` even when no `.g` source file changed. Incremental state
// must be invalidated on dependency or manifest edits.
#[test]
fn has_any_changed_detects_manifest_change() {
    let dir = TempDir::new().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_a = src_dir.join("a.g");
    fs::write(&file_a, "fn a() {}\n").unwrap();
    // Manifest present at the project root (the scan root passed below).
    let manifest = dir.path().join("glyim.toml");
    fs::write(&manifest, "name = \"demo\"\n").unwrap();

    let mut store = FingerprintStore::new();
    // Scan the project root so the manifest is fingerprinted alongside sources.
    store.update_all(dir.path(), "g").unwrap();
    assert!(
        !store.has_any_changed(dir.path(), "g", "").unwrap(),
        "no change should be reported right after update"
    );

    // Editing only the manifest (no .g change) must still flag a change.
    fs::write(&manifest, "name = \"demo-v2\"\ndependencies = []\n").unwrap();
    assert!(
        store.has_any_changed(dir.path(), "g", "").unwrap(),
        "manifest edit must invalidate incremental state"
    );
}

// Plan §4.2: a build-config flag change must invalidate the incremental cache
// even when no source file changed.
#[test]
fn has_any_changed_detects_build_config_change() {
    use crate::config::BuildOptions;

    let dir = TempDir::new().unwrap();
    let target = dir.path().join("target");
    fs::create_dir_all(&target).unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    let file_a = src_dir.join("a.g");
    fs::write(&file_a, "fn a() {}\n").unwrap();

    let debug_opts = BuildOptions {
        opt_level: 0,
        backend: "bytecode".to_string(),
        ..Default::default()
    };
    let release_opts = BuildOptions {
        opt_level: 2,
        backend: "bytecode".to_string(),
        ..Default::default()
    };
    let debug_hash = FingerprintStore::build_config_hash(&debug_opts);
    let release_hash = FingerprintStore::build_config_hash(&release_opts);
    assert_ne!(
        debug_hash, release_hash,
        "different opt-level must yield different hash"
    );

    // Record a successful debug build: set the active config, persist, then
    // reload so the *recorded* (last build) config is populated.
    let mut store = FingerprintStore::new();
    store.update_all(&src_dir, "g").unwrap();
    store.set_build_config(&debug_opts);
    store.save_to_dir(&target).unwrap();
    let store = FingerprintStore::load_from_dir(&target).unwrap();
    assert_eq!(
        store.recorded_config_hash(),
        Some(debug_hash.as_str()),
        "recorded config hash must survive a save/load round-trip"
    );

    // Same active config as the recorded one: no rebuild needed.
    assert!(
        !store.has_any_changed(&src_dir, "g", &debug_hash).unwrap(),
        "same build config must not invalidate cache"
    );

    // Bump the opt level: recorded (debug) vs active (release) differ, so the
    // whole cache is invalidated even though no source changed.
    assert!(
        store.has_any_changed(&src_dir, "g", &release_hash).unwrap(),
        "opt-level change must invalidate incremental cache"
    );

    // A backend change must also invalidate.
    let llvm_opts = BuildOptions {
        opt_level: 0,
        backend: "llvm".to_string(),
        ..Default::default()
    };
    let llvm_hash = FingerprintStore::build_config_hash(&llvm_opts);
    assert!(
        store.has_any_changed(&src_dir, "g", &llvm_hash).unwrap(),
        "backend change must invalidate incremental cache"
    );
}
