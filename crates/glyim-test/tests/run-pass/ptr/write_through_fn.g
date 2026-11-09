//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-RAW-PTR, STD-CORE-PTR, LANG-SYN-MUT, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn wr(d: *mut i32, s: i32) { unsafe { *d = s } }
fn main() -> i32 { let mut x = 0; let p = &mut x as *mut i32; unsafe { wr(p, 7) }; x }
