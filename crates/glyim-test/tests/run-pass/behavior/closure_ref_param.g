// Regression: a standalone closure with a reference-typed parameter
// annotation. `Pat::Binding` had no `ty` field, so `|x: &i32|` lost its
// annotation at HIR lowering; the typeck closure arm then only seeded
// params from an expected fn-sig, so a standalone closure got an unbound
// param and `*x` failed with "cannot dereference non-pointer type".
// Fixed by adding `param_tys` to `Expr::Closure`.
//@ mode: run-pass
//@ expect-exit: 6
//@ compile-flags: --with-stdlib
//@ feature: LANG-CLOSURE-EXPR, LANG-TY-REF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let f = |x: &i32| *x + 1;
    let n = 5;
    f(&n)
}
