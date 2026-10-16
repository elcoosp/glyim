// Regression: `ref mut` write on a nested Option payload; guards the
// temp-copy aliasing bug on a non-trivial scrutinee.
// test-mode: run-pass
// exit-code: 14
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let mut inner = Option::Some(7);
    match inner {
        Option::Some(ref mut v) => { *v = *v * 2; }
        Option::None => {}
    }
    inner.unwrap()
}
