//@ mode: run-pass
//@ expect-exit: 5
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-RAW-PTR, STD-CORE-PTR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { let x = 5; let p = &x as *const i32; unsafe { *p } }
