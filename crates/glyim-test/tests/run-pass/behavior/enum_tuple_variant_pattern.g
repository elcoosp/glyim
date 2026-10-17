// ANTI-REGRESSION / ANTI-MISCOMPILATION: pattern bindings inside a
// *tuple-style* enum variant must be matched positionally.
//
// Every field of `enum E { A(i32, i32) }` has an empty `Name`. The
// lowering's `field_index_by_name` lookup returned `Field(0)` for *every*
// binding, so `match E::A(3,4) { E::A(x,y) => y }` returned `3` instead
// of `4` — a silent wrong-value bug that passed any "does it run?"
// check.
//
// The fix consults the variant's `style`: a `Tuple` variant uses the
// source-order position as the field index. Distinct primes per field
// (and a 100/10/1 weighting) make any wrong binding produce a distinctly
// wrong exit code.
// test-mode: run-pass
// exit-code: 234
// compile-flags: --with-stdlib
// needs-llvm
enum E {
    A(i32, i32, i32),
    B,
}
fn main() -> i32 {
    let e = E::A(2, 3, 4);
    match e {
        E::A(x, y, z) => x * 100 + y * 10 + z,  // 234
        E::B => -1,
    }
}
