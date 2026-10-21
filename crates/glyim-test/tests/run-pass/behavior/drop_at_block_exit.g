// A value declared inside an inner block is destroyed at that block's `}`,
// not deferred to the enclosing function's return. The read of `N` after the
// block therefore observes the increment.
// test-mode: run-pass
// exit-code: 17
// compile-flags: --with-stdlib
static mut N: i32 = 0;
struct G { v: i32 }
impl Drop for G {
    fn drop(&mut self) { unsafe { N = N + 1; } }
}
fn main() -> i32 {
    {
        let _g = G { v: 1 };
    }
    unsafe { N + 16 }
}
