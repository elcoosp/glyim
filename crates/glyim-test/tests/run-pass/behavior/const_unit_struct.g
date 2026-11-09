// A bare zero-field struct name used as a value (`const G: Global =
// Global;`) folds to an empty aggregate. The const evaluator previously
// gave up on this path, which forced typeck to suppress *all* const-eval
// errors for ADT-typed consts — hiding genuine failures in user code.
//@ mode: run-pass
//@ expect-exit: 13
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-STRUCT, LANG-SYN-TUPLE-STRUCT, LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct Global;
const G: Global = Global;
fn main() -> i32 { let _g = G; 13 }
