// Nested block scopes drop innermost-first, each exactly once.
//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: COMPILER-OPT-DROP-ELAB, STD-CORE-OPS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
static mut N: i32 = 0;
struct G;
impl Drop for G {
    fn drop(&mut self) { unsafe { N = N + 1; } }
}
fn main() -> i32 {
    {
        let _a = G;
        {
            let _b = G;
        }
        let _c = G;
    }
    unsafe { N }
}
