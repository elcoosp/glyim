//@ ignore
// Companion for `main.g`; not a standalone fixture.
//@ mode: run-pass
//@ feature: LANG-MACRO-BUILTIN-INCLUDE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
//@ expect-exit: 0
pub fn helper() -> i32 { 41 }
