// ANTI-REGRESSION: `move` closures take their captures by value; the
// resulting environment shape must be identical to the by-ref case (only
// the capture operation differs).
// test-mode: run-pass
// exit-code: 71
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let n = 29;
    let f = move |x: i32| x + n;
    f(42)
}
