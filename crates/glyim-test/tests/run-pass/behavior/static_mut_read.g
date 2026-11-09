// Sanity: a `static mut` read without a preceding write returns the
// initializer value.
//@ mode: run-pass
//@ expect-exit: 19
//@ compile-flags: --with-stdlib
//@ feature: LANG-SCOPE-STATIC-ITEM, LANG-SYN-MUT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
static mut SEED: i32 = 19;
fn main() -> i32 { unsafe { SEED } }
