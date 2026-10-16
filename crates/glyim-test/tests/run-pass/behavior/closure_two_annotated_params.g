// Regression: closure param type annotations, mixed by-value and by-ref.
// Guards the HIR `Expr::Closure::param_tys` fix (annotations used to be
// dropped, leaving params as fresh unbound vars).
// test-mode: run-pass
// exit-code: 13
// compile-flags: --with-stdlib
fn main() -> i32 {
    let f = |x: i32, y: &i32| x + *y;
    let n = 3;
    f(10, &n)
}
