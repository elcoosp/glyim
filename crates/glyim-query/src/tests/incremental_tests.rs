use crate::incremental::IncrementalState;
use crate::fingerprint::Fingerprint;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn fresh_state_has_no_source_hashes() {
    let dir = TempDir::new().unwrap();
    let state = IncrementalState::load_or_create(dir.path());
    assert!(state.source_hashes().is_empty());
}

#[test]
fn record_source_hash() {
    let dir = TempDir::new().unwrap();
    let mut state = IncrementalState::load_or_create(dir.path());
    state.record_source("main.g", Fingerprint::of(b"source content"));
    assert_eq!(state.source_hashes().len(), 1);
    assert_eq!(
        state.source_hash("main.g"),
        Some(Fingerprint::of(b"source content"))
    );
}

#[test]
fn detect_changed_files() {
    let dir = TempDir::new().unwrap();
    let mut state = IncrementalState::load_or_create(dir.path());
    state.record_source("a.g", Fingerprint::of(b"old a"));
    state.record_source("b.g", Fingerprint::of(b"old b"));
    let changed = state.compute_changed_files(&[
        ("a.g", Fingerprint::of(b"new a")),
        ("b.g", Fingerprint::of(b"old b")),
    ]);
    assert_eq!(changed.len(), 1);
    assert!(changed.contains(&"a.g".to_string()));
}

#[test]
fn new_file_is_changed() {
    let dir = TempDir::new().unwrap();
    let mut state = IncrementalState::load_or_create(dir.path());
    state.record_source("a.g", Fingerprint::of(b"a"));
    let changed = state.compute_changed_files(&[
        ("a.g", Fingerprint::of(b"a")),
        ("c.g", Fingerprint::of(b"c")),
    ]);
    assert_eq!(changed.len(), 1);
    assert!(changed.contains(&"c.g".to_string()));
}

#[test]
fn deleted_file_is_detected() {
    let dir = TempDir::new().unwrap();
    let mut state = IncrementalState::load_or_create(dir.path());
    state.record_source("a.g", Fingerprint::of(b"a"));
    state.record_source("b.g", Fingerprint::of(b"b"));
    let deleted = state.compute_deleted_files(&["a.g"]);
    assert!(deleted.contains(&"b.g".to_string()));
}

#[test]
fn save_and_reload_preserves_state() {
    let dir = TempDir::new().unwrap();
    {
        let mut state = IncrementalState::load_or_create(dir.path());
        state.record_source("main.g", Fingerprint::of(b"content"));
        state.save().unwrap();
    }
    let state = IncrementalState::load_or_create(dir.path());
    assert_eq!(
        state.source_hash("main.g"),
        Some(Fingerprint::of(b"content"))
    );
}

#[test]
fn apply_changes_invalidates_queries() {
    let dir = TempDir::new().unwrap();
    let mut state = IncrementalState::load_or_create(dir.path());
    let file_fp = Fingerprint::of(b"main.g_content");
    let query_fp = Fingerprint::of(b"parse_main");
    let dep_fp = crate::Dependency::file("main.g", file_fp).fingerprint();
    state.ctx().dep_graph().write().unwrap().add_node(dep_fp);
    let dep_fp = crate::Dependency::file("main.g", file_fp).fingerprint();
    state.ctx().dep_graph().write().unwrap().add_node(dep_fp);
    state.ctx().insert(
        query_fp,
        Arc::new(42i64),
        Fingerprint::of(b"42"),
        vec![crate::Dependency::file("main.g", file_fp)],
    );
    state
        .ctx()
        .record_dependency(query_fp, crate::Dependency::file("main.g", file_fp));
    state.record_source("main.g", file_fp);
    assert!(state.ctx().is_green(&query_fp));
    state.ctx().invalidate_fingerprints(&[dep_fp]);
    assert!(!state.ctx().is_green(&query_fp));
}
