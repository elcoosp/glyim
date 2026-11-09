//@ mode: compile-pass
//@ feature: COMPILER-FRONT-DEFTABLE, TOOL-TEST-COMPILE-PASS
//@ backends: gam
struct Vec<T> { data: *mut T, len: usize, cap: usize }
impl<T> Vec<T> {
    fn push(&mut self, value: T) { }
}
fn main() {
    let mut v = Vec::<i32> { data: 0 as *mut i32, len: 0, cap: 0 };
    v.push(42);
}
