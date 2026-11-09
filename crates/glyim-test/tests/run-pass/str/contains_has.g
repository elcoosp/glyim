//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-STR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { if "abc".contains("b") { 1 } else { 0 } }
