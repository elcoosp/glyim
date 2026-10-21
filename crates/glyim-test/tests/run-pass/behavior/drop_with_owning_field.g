// A `Drop` type holding an owning `String`: running the destructor twice
// would double-free the string's buffer. The moved-from temp's drop flag
// must be cleared by the move.
// test-mode: run-pass
// exit-code: 31
// compile-flags: --with-stdlib
static mut COUNT: i32 = 0;
struct H { s: String }
impl Drop for H {
    fn drop(&mut self) { unsafe { COUNT = COUNT + 1; } }
}
fn work() { let _h = H { s: String::new() }; }
fn main() -> i32 {
    work();
    unsafe { COUNT + 30 }
}
