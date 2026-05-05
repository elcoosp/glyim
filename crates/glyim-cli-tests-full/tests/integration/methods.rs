#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_impl_method() {
    let src = "struct Point { x, y }\nimpl Point {\n    fn zero() -> Point { Point { x: 0, y: 0 } }\n}\nmain = () => { let p = Point::zero(); p.x }";
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 0);
}

#[test]
fn e2e_impl_method_chain() {
    let src = "struct Counter { val: i64 }
impl Counter {
    fn inc(mut self: Counter) -> Counter { self.val = self.val + 1; self }
}
main = () => { let c = Counter { val: 0 }; c.inc().inc().val }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "method chain: {:?}", result.err());
    assert_eq!(result.unwrap(), 2);
}

#[test]
fn e2e_generic_method_unwrap() {
    let src = "struct Wrapper<T> { value: T }
impl<T> Wrapper<T> {
    fn unwrap(self: Wrapper<T>) -> T { self.value }
}
main = () => { let w: Wrapper<i64> = Wrapper { value: 42 }; w.unwrap() }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "generic method unwrap: {:?}", result.err());
    assert_eq!(result.unwrap(), 42);
}

