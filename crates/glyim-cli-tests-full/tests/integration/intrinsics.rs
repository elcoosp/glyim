#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_size_of_i64() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => __size_of::<i64>()"), None).unwrap(),
        8
    );
}

#[test]
fn e2e_size_of_unit() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => __size_of::<()>()"), None).unwrap(),
        8
    );
}

#[test]
fn e2e_intrinsic_ptr_alloc() {
    let src = r#"
        main = () => {
            let ptr = __glyim_alloc(8);
            *(ptr as *mut i64) = 42;
            let val = *(ptr as *mut i64);
            __glyim_free(ptr as *mut u8);
            val
        }
    "#;
    let result = pipeline::run(&temp_g(src), None);
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn e2e_size_of_generic_struct() {
    let src = "struct Container<T> { value: T }
fn container_size() -> i64 { __size_of::<Container<i64>>() }
main = () => container_size()";
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 8);
}

#[test]
fn e2e_size_of_generic_function_param() {
    let src = "fn size_of_val<T>(x: T) -> i64 { __size_of::<T>() }
main = () => size_of_val(42)";
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 8);
}

#[test]
fn e2e_ptr_offset_builtin() {
    let src = "main = () => { let x = 1; let y = __ptr_offset(0 as *mut u8, x); 0 }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "__ptr_offset builtin: {:?}", result.err());
}

