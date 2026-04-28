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

#[test] fn e2e_let_binding() {
    assert_eq!(pipeline::run(&temp_xyz("main = () => { let x = 42 }")).unwrap(), 0);
}

#[test] fn e2e_let_mut_assign() {
    let input = temp_xyz("main = () => { let mut x = 10\nx = x + 5\nx }");
    assert_eq!(pipeline::run(&input).unwrap(), 15);
}

#[test] fn e2e_if_true_branch() {
    assert_eq!(pipeline::run(&temp_xyz("main = () => { if 1 { 10 } else { 20 } }")).unwrap(), 10);
}

#[test] fn e2e_if_false_branch() {
    assert_eq!(pipeline::run(&temp_xyz("main = () => { if 0 { 10 } else { 20 } }")).unwrap(), 20);
}

#[test] fn e2e_if_without_else() {
    assert_eq!(pipeline::run(&temp_xyz("main = () => { if 0 { 42 } }")).unwrap(), 0);
}

#[test] fn e2e_else_if_chain() {
    assert_eq!(pipeline::run(&temp_xyz("main = () => { if 0 { 1 } else if 0 { 2 } else { 3 } }")).unwrap(), 3);
}
