//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-RAW-PTR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { let x = 7; let p = &x as *const i32; unsafe { *p } }
