#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_bool() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if true { 10 } else { 20 } }"), None).unwrap(),
        10
    );
}

#[test]
fn e2e_float() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let x = 3.14; 1 }"), None).unwrap(),
        1
    );
}

#[test]
fn e2e_float_arithmetic_no_crash() {
    let src = "fn main() -> i64 { let x = 3.0; let y = x + 2.0; 1 }";
    assert!(pipeline::run(&temp_g(src), None).is_ok());
}

#[test]
fn e2e_float_add() {
    let src = "main = () => { let a = 2.5; let b = 3.5; a + b }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "float add compilation: {:?}", result.err());
}

#[test]
fn e2e_float_cmp() {
    let src = "main = () => { let a = 1.0; let b = 1.0; if a == b { 1 } else { 0 } }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "float cmp: {:?}", result.err());
    assert_eq!(result.unwrap(), 1);
}

