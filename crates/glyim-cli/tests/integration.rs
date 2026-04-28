use std::path::PathBuf;
use glyim_cli::pipeline;

fn temp_xyz(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.xyz");
    std::fs::write(&path, content).unwrap();
    Box::leak(Box::new(dir));
    path
}

#[test] fn e2e_main_42() { assert_eq!(pipeline::run(&temp_xyz("main = () => 42")).unwrap(), 42); }
#[test] fn e2e_add() { assert_eq!(pipeline::run(&temp_xyz("main = () => 1 + 2")).unwrap(), 3); }
#[test] fn e2e_block_last() { assert_eq!(pipeline::run(&temp_xyz("main = () => { 1 2 }")).unwrap(), 2); }
#[test] fn e2e_missing_main() { assert!(pipeline::run(&temp_xyz("fn other() { 1 }")).is_err()); }
#[test] fn e2e_parse_error() { assert!(pipeline::run(&temp_xyz("main = +")).is_err()); }
