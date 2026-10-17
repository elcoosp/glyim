// ANTI-REGRESSION / ANTI-MISCOMPILATION: `for x in [a, b, c]` must bind
// `x: T` by value and iterate every element.
//
// Two stacked bugs:
//   1. Typeck routed array iterables through the `Iterator` trait, whose
//      only matching impl is `impl Iterator for &[T]` — yielding
//      `Option<&T>` (8-byte pointer) into a `T`-sized (4-byte `i32`)
//      binding slot: ABI mismatch, SEGV on `s = s + x`. Fixed by
//      setting `next = None` and `item_ty = T` for array iterables.
//   2. Lowering's index-based loop emitted the idx/len init *inside*
//      `header_bb` (the loop target), so `idx = 0` reset every
//      iteration and `idx < len` stayed true forever — an infinite
//      loop. Fixed by hoisting the init into the pre-header `bb0`
//      before the `terminate(Goto { header_bb })`.
//
// Distinct prime sums across 5 elements make any residual off-by-one
// or skipped-element regression produce a distinctly wrong exit code.
// test-mode: run-pass
// exit-code: 15
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a = [1, 2, 3, 4, 5];
    let mut s = 0;
    for x in a { s = s + x; }
    // 1+2+3+4+5 = 15
    s
}
