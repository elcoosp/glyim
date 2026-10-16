// ANTI-REGRESSION: the *un-substituted* body of a generic function must not
// be emitted alongside its concrete instantiation.
//
// `MonoCtx::collect` used to enqueue the raw generic body of
// `Option::<T>::unwrap` (`return_ty = Param(0)`) alongside the substituted
// one. `fn_abi_of` then called `layout_of` on a `Param` and ICEd with
// `UnknownType(Ty(16))`. The pipeline now filters any body whose
// `return_ty` is `Param`/`Bound`/`Projection` — see
// `body_is_raw_generic` in glyim-pipeline.
//
// A failure here is a compile-time ICE:
// `[X0000] fn type lowering failed: fn_abi_of failed: UnknownType(...)`.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    // Uses `Option::<Option<i32>>::unwrap` and `Option::<i32>::unwrap` in
    // one chain so the *raw* generic template of `unwrap` would have been
    // enqueued for at least one of them at the old code.
    Option::Some(Option::Some(6)).unwrap().unwrap()
}
