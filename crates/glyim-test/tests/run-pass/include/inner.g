//@ ignore
//@ mode: run-pass
//@ feature: LANG-MACRO-BUILTIN-INCLUDE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
//@ expect-exit: 0
pub fn inner() -> i32 { 3 }
