#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_cast_int_to_float() {
    let _ = pipeline::run(&temp_g("main = () => 42 as f64"), None).unwrap();
}

#[test]
fn e2e_primitive_casts() {
    let src = "main = () => { let a: i64 = 42; let b: f64 = a as f64; let c: bool = true; let d: Str = \"hi\"; let e: *mut u8 = 0 as *mut u8; 0 }";
    let result = pipeline::run_jit(src);
    assert!(
        result.is_ok(),
        "primitive casts should compile and run: {:?}",
        result.err()
    );
}

