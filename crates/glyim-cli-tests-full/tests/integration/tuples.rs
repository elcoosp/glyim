#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_tuple() {
    let src = "main = () => { let p = (1, 2); p._0 }";
    let _result = pipeline::run(&temp_g(src), None).unwrap();
}

#[test]
fn e2e_tuple_mixed_types() {
    let src = "main = () => { let t = (1, true, 3.14); t._1 }";
    let result = pipeline::run_jit(src);
    // t._1 is a bool, which becomes i64 1
    assert!(result.is_ok(), "tuple mixed types: {:?}", result.err());
    // The value is a bool (true) which is represented as i64 1
    assert_eq!(result.unwrap(), 1);
}

#[test]
fn e2e_tuple_destructure() {
    let src = "main = () => { let (a, b) = (10, 20); a }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "tuple destructure: {:?}", result.err());
    assert_eq!(result.unwrap(), 10);
}

