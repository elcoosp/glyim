// An enum variant used as a value in a `const` initializer. This was the
// last shape blocked on the `is_primitive` const-eval gate: the evaluator
// could not resolve `C::B`, so the error was suppressed for ADT-typed
// consts and the non-folded const reached codegen as a `ConstRef`.
// test-mode: run-pass
// exit-code: 2
// compile-flags: --with-stdlib
enum C { A, B }
const K: C = C::B;
fn main() -> i32 { match K { C::A => 1, C::B => 2 } }
