// Regression: `Option::filter` with a predicate that returns false — the
// result is `None`, so `unwrap_or` must return its default. Guards the
// stdlib filter impl + the ref-pattern + closure-param fixes it depends on.
// test-mode: run-pass
// exit-code: 9
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let o = Option::Some(5);
    let r = o.filter(|x| *x > 100);
    r.unwrap_or(9)
}
