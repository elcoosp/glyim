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
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let mut x = 10\nx = x + 5\nx }")).unwrap(),
        15
    );
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
    let _ = pipeline::run(&temp_g("main = () => { println(42) }")).expect("println_int");
}
#[test]
fn e2e_println_string_compiles_and_runs() {
    let _ = pipeline::run(&temp_g(r#"main = () => { println("hello") }"#)).expect("println_string");
}
#[test]
fn e2e_assert_pass() {
    let _ = pipeline::run(&temp_g("main = () => { assert(1 == 1) }")).expect("assert_pass");
}
#[test]
fn e2e_assert_fail_exits_nonzero() {
    assert_ne!(
        pipeline::run(&temp_g("main = () => { assert(0) }")).unwrap(),
        0
    );
}
#[test]
fn e2e_assert_fail_msg_exits_nonzero() {
    assert_ne!(
        pipeline::run(&temp_g(r#"main = () => { assert(0, "oops") }"#)).unwrap(),
        0
    );
}
#[test]
fn e2e_bool_literal() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if true { 10 } else { 20 } }")).unwrap(),
        10
    );
}
#[test]
fn e2e_float_literal() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let x = 3.14; 1 }")).unwrap(),
        1
    );
}
#[test]
fn e2e_enum_variant() {
    assert_eq!(
        pipeline::run(&temp_g(
            "enum Color { Red, Green, Blue }\nmain = () => { let c = Color::Green; 1 }"
        ))
        .unwrap(),
        1
    );
}
#[test]
fn e2e_struct_literal_and_access() {
    assert_eq!(
        pipeline::run(&temp_g(
            "struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; 42 }"
        ))
        .unwrap(),
        42
    );
}
#[test]
fn e2e_match_expression() {
    assert_eq!(pipeline::run(&temp_g("enum Color { Red, Green, Blue }\nmain = () => { let c = Color::Red; match c { Color::Red => 1, Color::Green => 2, Color::Blue => 3 } }")).unwrap(), 1);
}
#[test]
fn e2e_some_and_none() {
    assert_eq!(
        pipeline::run(&temp_g(
            "main = () => { let m = Some(42); match m { Some(v) => v, None => 0 } }"
        ))
        .unwrap(),
        42
    );
}
#[test]
fn e2e_ok_and_err() {
    assert_eq!(
        pipeline::run(&temp_g(
            "main = () => { let r = Ok(42); match r { Ok(v) => v, Err(_) => 0 } }"
        ))
        .unwrap(),
        42
    );
}
#[test]
fn e2e_macro_identity() {
    assert_eq!(
        pipeline::run(&temp_g(
            "@identity fn transform(expr: Expr) -> Expr { return expr } main = () => @identity(99)"
        ))
        .unwrap(),
        99
    );
}
#[test]
#[test]
fn e2e_arrow_operator() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let r = Ok(42)?; r }")).unwrap(),
        42
    );
}
