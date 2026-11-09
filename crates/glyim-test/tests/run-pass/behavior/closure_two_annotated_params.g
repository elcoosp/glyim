// Regression: closure param type annotations, mixed by-value and by-ref.
// Guards the HIR `Expr::Closure::param_tys` fix (annotations used to be
// dropped, leaving params as fresh unbound vars).
//@ mode: run-pass
//@ expect-exit: 13
//@ compile-flags: --with-stdlib
//@ feature: LANG-CLOSURE-EXPR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let f = |x: i32, y: &i32| x + *y;
    let n = 3;
    f(10, &n)
}
