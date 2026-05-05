#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_struct() {
    assert_eq!(
        pipeline::run(
            &temp_g("struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; 42 }"),
            None
        )
        .unwrap(),
        42
    );
}

#[test]
fn e2e_struct_with_ptr_parse_and_typecheck() {
    let src = "struct Ptr { data: *mut i64 }\nmain = () => { 42 }";
    assert!(pipeline::run(&temp_g(src), None).is_ok());
}

#[test]
fn e2e_struct_mixed_types() {
    let src = "struct Mixed { a: i64, b: bool, c: f64 }
main = () => { let m = Mixed { a: 10, b: true, c: 3.14 }; m.a }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "mixed struct: {:?}", result.err());
    assert_eq!(result.unwrap(), 10);
}

#[test]
fn e2e_nested_struct() {
    let src = "struct Inner { x: i64 }
struct Outer { inner: Inner }
main = () => { let o = Outer { inner: Inner { x: 42 } }; o.inner.x }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "nested struct: {:?}", result.err());
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn e2e_let_struct_pattern() {
    let src = "struct Point { x, y }
main = () => { let p = Point { x: 10, y: 20 }; let Point { x, y } = p; x }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "let struct pattern: {:?}", result.err());
    assert_eq!(result.unwrap(), 10);
}

