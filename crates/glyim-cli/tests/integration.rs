use glyim_cli::pipeline;
use std::path::PathBuf;

fn temp_g(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.g");
    std::fs::write(&path, content).unwrap();
    Box::leak(Box::new(dir));
    path
}

#[test]
fn e2e_main_42() {
    assert_eq!(pipeline::run(&temp_g("main = () => 42")).unwrap(), 42);
}
#[test]
fn e2e_add() {
    assert_eq!(pipeline::run(&temp_g("main = () => 1 + 2")).unwrap(), 3);
}
#[test]
fn e2e_block_last() {
    assert_eq!(pipeline::run(&temp_g("main = () => { 1 2 }")).unwrap(), 2);
}
#[test]
fn e2e_missing_main() {
    assert!(pipeline::run(&temp_g("fn other() { 1 }")).is_err());
}
#[test]
fn e2e_parse_error() {
    assert!(pipeline::run(&temp_g("main = +")).is_err());
}

#[test]
fn e2e_let_binding() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let x = 42 }")).unwrap(),
        0
    );
}

#[test]
fn e2e_let_mut_assign() {
    let input = temp_g("main = () => { let mut x = 10\nx = x + 5\nx }");
    assert_eq!(pipeline::run(&input).unwrap(), 15);
}

#[test]
fn e2e_if_true_branch() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if 1 { 10 } else { 20 } }")).unwrap(),
        10
    );
}

#[test]
fn e2e_if_false_branch() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if 0 { 10 } else { 20 } }")).unwrap(),
        20
    );
}

#[test]
fn e2e_if_without_else() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if 0 { 42 } }")).unwrap(),
        0
    );
}

#[test]
fn e2e_else_if_chain() {
    assert_eq!(
        pipeline::run(&temp_g(
            "main = () => { if 0 { 1 } else if 0 { 2 } else { 3 } }"
        ))
        .unwrap(),
        3
    );
}

#[test]
fn e2e_println_int_compiles_and_runs() {
    let input = temp_g("main = () => { println(42) }");
    let _ = pipeline::run(&input).expect("println_int should compile and run");
}

#[test]
fn e2e_println_string_compiles_and_runs() {
    let input = temp_g(r#"main = () => { println("hello") }"#);
    let _ = pipeline::run(&input).expect("println_string should compile and run");
}

#[test]
fn e2e_assert_pass() {
    let input = temp_g("main = () => { assert(1 == 1) }");
    let _ = pipeline::run(&input).expect("assert_pass should compile and run");
}

#[test]
fn e2e_assert_fail_exits_nonzero() {
    let input = temp_g("main = () => { assert(0) }");
    let code = pipeline::run(&input).unwrap();
    assert_ne!(code, 0, "assert failure should exit non-zero");
}

#[test]
fn e2e_assert_fail_msg_exits_nonzero() {
    let input = temp_g(r#"main = () => { assert(0, "oops") }"#);
    let code = pipeline::run(&input).unwrap();
    assert_ne!(code, 0, "assert with message should exit non-zero");
}
