// ANTI-REGRESSION: a method call on a receiver whose type is still an
// `Infer(TyVar)` at the point `resolve_method_call` returns must still
// resolve to the right `(FnDefId, Substitution)`.
//
// The *inner* `.unwrap()` in `Some(Some(6)).unwrap().unwrap()` sees a
// receiver type of `Infer(TyVar(..))` (the pre-unification snapshot), even
// though `resolve_method_call` has already unified it against
// `Option<i32>`. The `Static`/`Builtin` arms now call
// `infer.resolve_ty_shallow` before reading the receiver's ADT args —
// see commit "fix(typeck): resolve receiver through infer table when
// extracting callee substs". Without it, `impl_part` came back empty and
// the call site emitted `FnDef(362, [])` while the definition was
// `FnDef(362, [Option<i32>])`.
//
// A failure here is a link error (`___glyim_fn_362`) — or, if the symbol
// happened to match, a wrong-ABI call.
// test-mode: run-pass
// exit-code: 4
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    // The receiver of the second `.unwrap` is the *result* of the first
    // (type still infer at the read site).
    let o: Option<Option<i32>> = Option::Some(Option::Some(4));
    o.unwrap().unwrap()
}
