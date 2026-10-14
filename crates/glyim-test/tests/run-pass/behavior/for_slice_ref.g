// `for x in &slice`: iterate a `&[T]` by reference. Exercises the
// `IntoIterator`/`Iterator for &[T]` path plus the deferred `SizeOf`
// element stride in the dynamic slice range.
// needs-llvm
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = [1, 2, 3];
    let s: &[i32] = &a;
    let mut n = 0;
    for x in s { n = n + *x; }
    n
}
