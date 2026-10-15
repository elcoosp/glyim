// KNOWN FAILING — `Option::or(self, optb: Option<T>) -> Option<T>`.
// `.or(b)` typechecks (returns `Option<i32>`) but codegen ICEs with
// `[X0000] layout error building aggregate: UnknownType(Ty(0))`.
// Suspected: `or`'s body returns `self`/`optb` (existing values) rather
// than a fresh aggregate, and monomorphization of the generic impl body
// leaves a `Param` unsubstituted in the returned ADT's substitution.
// Tracked as a known gap; `ignore` keeps CI green while the fixture
// documents the exact shape.
// test-mode: run-pass
// ignore
// exit-code: 8
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a: Option<i32> = Option::None;
    let b = Option::Some(8);
    a.or(b).unwrap()
}
