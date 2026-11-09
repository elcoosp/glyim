//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-RAW-PTR, STD-CORE-PTR, LANG-SYN-MUT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { let mut x = 0; let p = &mut x as *mut i32; unsafe { *p = 7 }; x }
