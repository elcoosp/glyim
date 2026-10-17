// ANTI-REGRESSION: using the parsed value (rather than discarding it)
// must work — the previous session's `"X".parse::<T>()` fix covered the
// discard-only case; matching on the parsed `Result` then tripped a
// separate bug in the `Expr::MethodCall` return type.
//
// The `MethodDispatch::Static` arm recomputes `ret_ty` from the callee's
// final substitution (`Result<i32, ParseIntError>`), but the *outer*
// `ret_ty` from `resolve_method_call` — which the expression's return
// slot reported — still carried the pre-substitution shape
// (`Result<T, <T as FromStr>::Err>` with a rigid `T`). That made
// `let r = "42".parse::<i32>();` bind `r` to the unsubstituted shape
// while the call's destination local was the resolved one; the later
// `match r { Err(_) => .. }` then tried to lay out the unnormalized
// projection and ICEd with:
//
//     internal compiler error:
//     TyKind::Projection reached LLVM codegen – type normalization incomplete
//
// The fix returns `thir_expr.ty` (the dispatch arm's precise type) from
// the MethodCall arm rather than the outer `ret_ty`, so the bound local
// and the destination local agree.
// test-mode: run-pass
// exit-code: 42
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let r = "42".parse::<i32>();
    match r {
        Result::Ok(v) => v,
        Result::Err(_) => -1,
    }
}
