// KNOWN_GAPS §3.9 (this session): `&[T].into_iter().next()` failed typeck
// ("cannot dereference non-pointer type") because the impl's
// `Option<Self::Item>` return type reached the call site as the raw
// `<Self as Iterator>::Item` projection instead of the impl's concrete
// `&'a T`. The projection now normalizes through the impl's registered
// associated-type table with a deep impl-param substitution.
// test-mode: run-pass
// exit-code: 10
// Interp note: same fat-aggregate limitation as slice_inherent_first_last.g.
// needs-llvm
// compile-flags: --with-stdlib
fn main() -> i32 {
    let arr = [10, 20, 30];
    let s: &[i32] = &arr;
    let mut it = s.into_iter();
    match it.next() {
        Option::Some(x) => *x,
        Option::None => 0,
    }
}
