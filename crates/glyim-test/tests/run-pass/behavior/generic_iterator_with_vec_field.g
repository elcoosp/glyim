// A user-defined `impl<T> Iterator for VecIter<T>` whose `next` reads a
// `Vec<T>` field by move (`self.v[self.i]`).
//
// Two separate fixes are exercised here:
//
// 1. `Place::ty` (glyim-mir) and codegen's `place_ptr` now handle
//    `Deref` on `Vec<T>` (AdtId 1020) as a *trait* deref — the MIR
//    chain `[Deref, Field(0), Deref, Index(..)]` produced by
//    `self.v[self.i]` used to fall into the generic
//    `Deref`-on-non-pointer arm (Place::ty → ERROR; codegen → ICE
//    "Found StructValue … but expected PointerValue"). The new arms
//    read the Vec's `{ptr, cap, len}` header into a `{ptr, len}` slice
//    fat pointer, then the `Index` arm classifies the element as `T`.
//
// 2. The recursive `TypeLookup::subst_ty` fix (§3.4 Bug A) is a
//    prerequisite: `self.v` for `VecIter<T>` is
//    `Adt(1020, [Param(0)])` and would otherwise leak `Param(0)` into
//    codegen.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
struct VecIter<T> { v: Vec<T>, i: usize }

impl<T> Iterator for VecIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.i < self.v.len() {
            let x = self.v[self.i];
            self.i = self.i + 1;
            Option::Some(x)
        } else {
            Option::None
        }
    }
}

fn main() -> i32 {
    let v: Vec<i32> = [1, 2, 3].to_vec();
    let mut it = VecIter { v: v, i: 0 };
    let mut sum = 0;
    loop {
        match it.next() {
            Option::Some(x) => sum = sum + x,
            Option::None => break,
        }
    }
    sum
}
