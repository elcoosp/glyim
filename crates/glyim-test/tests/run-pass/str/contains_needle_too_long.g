//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-STR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { if "ab".contains("abc") { 1 } else { 0 } }
