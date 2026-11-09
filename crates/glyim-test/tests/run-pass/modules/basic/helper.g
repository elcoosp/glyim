//@ ignore
//@ mode: run-pass
//@ feature: LANG-SCOPE-MOD-DECL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
//@ expect-exit: 0
pub fn value() -> i32 { 42 }
