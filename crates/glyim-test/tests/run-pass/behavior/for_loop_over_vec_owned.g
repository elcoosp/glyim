// `for x in v` where `v: Vec<T>` (by-value). Typeck resolves
// `IntoIterator::into_iter` for `Vec<T>` via an exact-receiver matcher
// (see check_expr.rs `resolve_into_iter_exact`), substitutes the Vec's
// element into the impl's stored output template, and uses
// `VecIntoIter<i32>` as the effective iterable for the `next` scan.
// Lowering emits the `into_iter(v)` call into a fresh local and rebinds
// `iter_local` to it — avoiding the 3-word Vec vs 2-word slice-pointer
// layout mismatch that segfaulted earlier attempts.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let v: Vec<i32> = [1, 2, 3].to_vec();
    let mut sum = 0;
    for x in v {
        sum = sum + x;
    }
    sum
}
