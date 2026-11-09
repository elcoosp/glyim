// A `Drop` type holding an owning `String`: running the destructor twice
// would double-free the string's buffer. The moved-from temp's drop flag
// must be cleared by the move.
//@ mode: run-pass
//@ expect-exit: 31
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-FIELD-ACCESS, COMPILER-OPT-DROP-ELAB, STD-CORE-OPS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
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
