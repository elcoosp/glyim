//@ mode: run-pass
//@ expect-exit: 4
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-ARRAY, LANG-TY-SLICE, STD-CORE-SLICE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { let a = [1, 2, 3, 4]; let s = &a[0..4]; s.len() as i32 }
