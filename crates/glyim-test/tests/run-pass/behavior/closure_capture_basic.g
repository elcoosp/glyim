// ANTI-REGRESSION: capturing closures must compile and run.
//
// Three stacked bugs fixed here:
//   1. Codegen's `place_ptr` `Field` arm only handled `TyKind::Adt`, so
//      a closure-field projection (capture read) fell through to
//      `Ty::ERROR`. Fixed by routing `TyKind::Closure` through the same
//      field-shape logic as `layout_of`.
//   2. The closure body is emitted *after* `main`, so the call site's
//      `module.get_function` returned `None` and the call arm bailed
//      with "closure body function was not emitted before the call
//      site". Fixed by forward-declaring the closure function with its
//      `FnSig`-derived type when not yet present.
//   3. The `MirConstKind::Fn` const operand mangled the closure's
//      re-typed `FnDefId` (really a `ClosureId`) to
//      `__glyim_fn_<id>_<subst_idx>_<len>` — a symbol that never gets
//      defined. Fixed by special-casing closures to use the un-mangled
//      form.
// test-mode: run-pass
// exit-code: 67
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a = 11;
    let b = 13;
    let f = |x: i32| x + a + b;
    f(43)
}
