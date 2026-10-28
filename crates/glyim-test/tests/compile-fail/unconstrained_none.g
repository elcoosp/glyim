// §3.5.24 Bug A: an unconstrained `Option::None` must be a typeck error
// ("type annotations needed"), not an LLVM codegen ICE.
//
// Before this fix, `let b = Option::None;` type-checked with
// `Adt1010<<error>>` in its substitution, zonk replaced the unbound
// `Infer(Ty)` with `Ty::ERROR`, and LLVM rejected it:
//   [X0000] Codegen error: Attempted to lower TyKind::Error to LLVM.
// The walker that collects unbound-infer spans
// (`walk_body_for_variant_ctor_calls` in `zonk.rs`) only covered
// `VariantCtor` calls (data-carrying variants), not `VariantRef` values
// (unit variants). Adding the `VariantRef` arm routes the span into the
// existing E0282 pipeline, which emits this clean diagnostic instead.
// test-mode: compile-fail
fn main() -> i32 {
    let b = Option::None; //~ ERROR type annotations needed
    let _ = b;
    0
}
