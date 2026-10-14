// `for i in 0..5` — unsuffixed bounds default to `Range<i32>`. The for-loop
// desugar resolves `IntoIterator::into_iter` when the iterable has no direct
// `Iterator` impl, and `Range<i32>` (declared before `Range<usize>` in
// iter.g so an unresolved element binds to `i32`) provides one.
// test-mode: run-pass
// exit-code: 10
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut n = 0;
    for i in 0..5 { n = n + i; }
    n
}
