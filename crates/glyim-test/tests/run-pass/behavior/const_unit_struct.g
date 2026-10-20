// A bare zero-field struct name used as a value (`const G: Global =
// Global;`) folds to an empty aggregate. The const evaluator previously
// gave up on this path, which forced typeck to suppress *all* const-eval
// errors for ADT-typed consts — hiding genuine failures in user code.
// test-mode: run-pass
// exit-code: 13
// compile-flags: --with-stdlib
struct Global;
const G: Global = Global;
fn main() -> i32 { let _g = G; 13 }
