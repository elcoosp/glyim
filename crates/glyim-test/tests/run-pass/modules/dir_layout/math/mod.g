//@ ignore
//@ mode: run-pass
//@ feature: LANG-SCOPE-MOD-DECL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
//@ expect-exit: 0
pub fn triple(x: i32) -> i32 { x + x + x }
