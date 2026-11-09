// A `static` value read in the middle of an arithmetic expression, to
// catch any miscompile that folds the read to a placeholder.
//@ mode: run-pass
//@ expect-exit: 59
//@ compile-flags: --with-stdlib
//@ feature: LANG-SCOPE-STATIC-ITEM, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
static BASE: i32 = 13;
fn main() -> i32 { BASE + BASE * 3 + 7 }
