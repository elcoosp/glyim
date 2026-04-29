use glyim_pkg::workspace::*;
use std::fs;

#[test]
fn detect_workspace_finds_root() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("glyim.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .unwrap();
    let crates_a = dir.path().join("crates").join("a");
    fs::create_dir_all(&crates_a).unwrap();
    fs::write(
        crates_a.join("glyim.toml"),
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let ws = detect_workspace(dir.path()).unwrap();
    assert_eq!(ws.root, dir.path());
    assert_eq!(ws.members.len(), 1);
    assert_eq!(ws.members[0], crates_a);
}

#[test]
fn detect_workspace_not_found() {
    let dir = tempfile::tempdir().unwrap();
    assert!(detect_workspace(dir.path()).is_none());
}

#[test]
fn detect_workspace_no_workspace_section() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("glyim.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    assert!(detect_workspace(dir.path()).is_none());
}

#[test]
fn detect_workspace_glob_excludes_non_toml_dir() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("glyim.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .unwrap();
    let a_dir = dir.path().join("crates").join("a");
    fs::create_dir_all(&a_dir).unwrap();
    fs::write(
        a_dir.join("glyim.toml"),
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    // b_dir exists but has no glyim.toml
    let b_dir = dir.path().join("crates").join("b");
    fs::create_dir_all(&b_dir).unwrap();
    let ws = detect_workspace(dir.path()).unwrap();
    assert_eq!(ws.members.len(), 1);
    assert_eq!(ws.members[0], a_dir);
}
