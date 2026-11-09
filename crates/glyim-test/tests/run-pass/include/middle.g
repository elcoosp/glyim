//@ ignore
//@ mode: run-pass
//@ feature: LANG-MACRO-BUILTIN-INCLUDE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
//@ expect-exit: 0
include!("inner.g");
pub fn middle() -> i32 { inner() * 2 }
