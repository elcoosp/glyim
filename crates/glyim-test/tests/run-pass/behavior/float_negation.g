// ANTI-REGRESSION: float-literal negation and float-to-int casts must
// round-trip through the constant folder without losing the cast.
//
// Two stacked bugs fixed here:
//   1. `-3.5` failed typeck with "cannot apply unary negation to
//      non-numeric type" because the check's `matches!` clause had
//      `Infer(Int)` and `Infer(Ty)` but was missing `Infer(Float)`.
//   2. `glyim-opt`'s `evaluate_rvalue_to_const` folded `Rvalue::Cast`
//      down to its operand's constant (`-3.5 as i32` became
//      `FloatBits(-3.5)`), so the surrounding `+` / `&` saw a float
//      where an `i32` was expected and codegen ICEd with "add:
//      mismatched types" / "bitand: expected integer types".
// test-mode: run-pass
// exit-code: 253
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let f = -3.5;
    (f as i32) & 255
}
