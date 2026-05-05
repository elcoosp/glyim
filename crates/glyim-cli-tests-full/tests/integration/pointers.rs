#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_deref_generic_ptr() {
    let src = "fn deref_generic<T>(p: *mut T) -> T { *p }
main = () => {
    let ptr = __glyim_alloc(8) as *mut i64;
    *ptr = 123;
    deref_generic(ptr)
}";
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 123);
}

