//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-STR, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { if "abc".trim().len() == 3 { 1 } else { 0 } }
