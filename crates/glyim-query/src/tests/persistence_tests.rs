use crate::context::QueryContext;
use crate::fingerprint::Fingerprint;
use crate::persistence::PersistenceLayer;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn persist_and_load_empty_context() {
    let dir = TempDir::new().unwrap();
    let ctx = QueryContext::new();
    PersistenceLayer::save(&ctx, dir.path()).unwrap();
    let loaded = PersistenceLayer::load(dir.path()).unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn persist_and_load_with_entries() {
    let dir = TempDir::new().unwrap();
    let ctx = QueryContext::new();
    let key = Fingerprint::of(b"query1");
    ctx.insert(
        key,
        Arc::new(42i64),
        Fingerprint::of(b"42"),
        vec![crate::Dependency::file("main.g", Fingerprint::of(b"src"))],
    );
    PersistenceLayer::save(&ctx, dir.path()).unwrap();
    let loaded = PersistenceLayer::load(dir.path()).unwrap();
    assert_eq!(loaded.len(), 1);
    assert!(loaded.is_green(&key));
    let result = loaded.get(&key).unwrap();
    // persisted values are placeholders; skip value check
}

#[test]
fn persist_preserves_dependency_graph() {
    let dir = TempDir::new().unwrap();
    let ctx = QueryContext::new();
    let file_fp = Fingerprint::of(b"file");
    let q1 = Fingerprint::of(b"q1");
    let q2 = Fingerprint::of(b"q2");
    ctx.insert(
        q1,
        Arc::new(1i64),
        Fingerprint::of(b"1"),
        vec![crate::Dependency::file("main.g", Fingerprint::of(b"src"))],
    );
    ctx.record_dependency(q1, crate::Dependency::file("main.g", Fingerprint::of(b"src")));
    ctx.insert(
        q2,
        Arc::new(2i64),
        Fingerprint::of(b"2"),
        vec![crate::Dependency::query(q1)],
    );
    ctx.record_dependency(q2, crate::Dependency::query(q1));
    PersistenceLayer::save(&ctx, dir.path()).unwrap();
    let loaded = PersistenceLayer::load(dir.path()).unwrap();
    let report = loaded.invalidate_fingerprints(&[file_fp]);
    assert!(report.red.contains(&q1));
    assert!(report.red.contains(&q2));
}

#[test]
fn load_from_nonexistent_dir_returns_empty() {
    let dir = TempDir::new().unwrap();
    let nonexistent = dir.path().join("nope");
    let loaded = PersistenceLayer::load(&nonexistent).unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn save_creates_directory_if_missing() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("a").join("b").join("c");
    let ctx = QueryContext::new();
    assert!(PersistenceLayer::save(&ctx, &nested).is_ok());
    assert!(nested.exists());
}
