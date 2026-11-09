//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-ARRAY, LANG-TY-SLICE, STD-CORE-SLICE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { let a = [1, 2, 3]; a.len() as i32 }
