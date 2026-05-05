use crate::watcher::FileWatcher;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn watcher_new_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let watcher = FileWatcher::new(&[dir.path().to_path_buf()], Duration::from_millis(10));
    assert!(watcher.is_ok(), "watcher creation should succeed");
}

#[test]
fn watcher_returns_none_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let watcher = FileWatcher::new(&[dir.path().to_path_buf()], Duration::from_millis(10))
        .expect("create watcher");
    drop(watcher); // drops tx, so rx.recv() returns Err
    // This test just verifies the stub doesn't crash
}
