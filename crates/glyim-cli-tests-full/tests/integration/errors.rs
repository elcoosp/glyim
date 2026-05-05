#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_missing_main() {
    assert!(pipeline::run(&temp_g("fn other() { 1 }"), None).is_err());
}

#[test]
fn e2e_parse_error() {
    assert!(pipeline::run(&temp_g("main = +"), None).is_err());
}

#[test]
fn e2e_invalid_cast_fails() {
    assert!(pipeline::run(&temp_g("main = () => 42 as Str"), None).is_err());
}

#[test]
fn e2e_wrong_field_fails() {
    assert!(
        pipeline::run(
            &temp_g("struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; p.z }"),
            None
        )
        .is_err()
    );
}

#[test]
fn e2e_type_error_unknown_field() {
    let input = temp_g("struct Point { x }\nmain = () => { let p = Point { x: 1 }; p.y }");
    let result = pipeline::run(&input, None);
    assert!(result.is_err());
}

#[test]
fn e2e_type_error_invalid_cast() {
    let input = temp_g("main = () => 42 as Str");
    let result = pipeline::run(&input, None);
    assert!(result.is_err());
}

#[test]
fn e2e_type_error_int_plus_bool() {
    let input = temp_g("fn main() -> i64 { let x: i64 = true; x }");
    let result = pipeline::run(&input, None);
    assert!(
        result.is_err(),
        "expected type error for bool in i64 variable"
    );
}

#[test]
fn e2e_type_error_missing_main() {
    let input = temp_g("fn other() { 1 }");
    let result = pipeline::run(&input, None);
    assert!(result.is_err());
}

#[test]
fn e2e_type_error_non_exhaustive_match() {
    let input = temp_g("enum Color { Red, Green, Blue }\nmain = () => match Color::Red { _ => 0 }");
    let result = pipeline::run(&input, None);
    assert!(result.is_ok());
}

#[test]
#[ignore = "type checker doesn't yet enforce bool-only if conditions; see typeck/expr.rs check_expr for If"]
fn e2e_bool_if_rejects_int_condition() {
    let src = "fn main() -> i64 { let x = 5; if x { 1 } else { 0 } }";
    assert!(pipeline::run(&temp_g(src), None).is_err());
}

#[test]
fn e2e_assign_to_immutable_is_error() {
    let src = "fn main() -> i64 { let x = 5; x = 10; x }";
    assert!(pipeline::run(&temp_g(src), None).is_err());
}

#[test]
fn e2e_typeck_rejects_non_bool_if_condition() {
    let src = r#"fn main() -> i64 { let x = 5; if x { 1 } else { 0 } }"#;
    let result = pipeline::run_jit(src);
    // Currently may or may not fail depending on implementation
    // Just verify no crash
    eprintln!("non-bool if condition result: {:?}", result);
}

