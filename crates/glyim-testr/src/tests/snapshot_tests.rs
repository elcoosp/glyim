use crate::snapshot::SnapshotStore;
use std::fs;
use std::path::PathBuf;

#[test]
fn new_snapshot_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path().to_path_buf());
    let name = "test1";
    let content = "hello world";
    store.assert_snapshot(name, content).expect("first snapshot");
    let snap_path = dir.path().join("test1.snap");
    assert!(snap_path.exists());
    let written = fs::read_to_string(&snap_path).unwrap();
    assert_eq!(written, content);
}

#[test]
fn matching_snapshot_passes() {
    let dir = tempfile::tempdir().unwrap();
    let name = "test2";
    fs::write(dir.path().join("test2.snap"), "foo bar").unwrap();
    let store = SnapshotStore::new(dir.path().to_path_buf());
    assert!(store.assert_snapshot(name, "foo bar").is_ok());
}

#[test]
fn mismatched_snapshot_fails() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("test3.snap"), "original").unwrap();
    let store = SnapshotStore::new(dir.path().to_path_buf());
    let result = store.assert_snapshot("test3", "different");
    assert!(result.is_err());
}
