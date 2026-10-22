// Indexing into a `Vec<T>` that is a *field* of a generic struct.
//
// `wrap.v` for `struct Wrap<T> { v: Vec<T>, i: usize }` is
// `Adt(1020, [Param(0)])` — a nested `Param`. Both `Place::ty` and
// codegen's `place_ptr` previously substituted only a top-level
// `Param`, leaving `Param(0)` in place; the subsequent `Index`
// projection then produced a bare `Param(0)` that `llvm_type_for_ty`
// rejected ("monomorphization should have resolved this"). The
// substitution is now recursive via `TypeLookup::subst_ty` (leaf-only
// default) overridden on the frozen `TyCtx`.
// test-mode: run-pass
// exit-code: 20
// compile-flags: --with-stdlib
struct Wrap<T> { v: Vec<T>, i: usize }
fn main() -> i32 {
    let w = Wrap { v: [10, 20, 30].to_vec(), i: 1 };
    w.v[w.i]
}
