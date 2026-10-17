// ANTI-REGRESSION / ANTI-MISCOMPILATION: a float-to-int cast feeding an
// integer binary op must survive constant propagation.
//
// `glyim-opt`'s constant folder previously returned the *operand's*
// constant for `Rvalue::Cast`, silently dropping the cast. This fixture
// tests a case where the cast result (`3`) differs from the operand
// (`3.99`) so a folded-away cast yields a visibly wrong value.
// test-mode: run-pass
// exit-code: 23
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let f = 3.99;
    (f as i32) + 20
}
