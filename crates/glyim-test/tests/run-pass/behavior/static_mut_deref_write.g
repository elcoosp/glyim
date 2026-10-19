// Deref + Field write through a `&mut` borrowed from a static mut.
// test-mode: run-pass
// exit-code: 31
// compile-flags: --with-stdlib
struct S { v: i32 }
static mut S_OBJ: S = S { v: 5 };
fn main() -> i32 {
    unsafe {
        let p: &mut S = &mut S_OBJ;
        (*p).v = 31;
        S_OBJ.v
    }
}
