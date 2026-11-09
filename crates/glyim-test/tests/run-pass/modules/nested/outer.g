//@ ignore
//@ mode: run-pass
//@ feature: LANG-SCOPE-MOD-DECL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
//@ expect-exit: 0
mod inner;
pub fn via_inner() -> i32 { inner::seed() }
