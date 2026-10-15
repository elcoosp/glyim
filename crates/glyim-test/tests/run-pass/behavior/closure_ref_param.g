// Regression: a standalone closure with a reference-typed parameter
// annotation. `Pat::Binding` had no `ty` field, so `|x: &i32|` lost its
// annotation at HIR lowering; the typeck closure arm then only seeded
// params from an expected fn-sig, so a standalone closure got an unbound
// param and `*x` failed with "cannot dereference non-pointer type".
// Fixed by adding `param_tys` to `Expr::Closure`.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let f = |x: &i32| *x + 1;
    let n = 5;
    f(&n)
}
