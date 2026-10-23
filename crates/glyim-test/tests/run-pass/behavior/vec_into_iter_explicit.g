// `v.into_iter()` — the `IntoIterator for Vec<T>` impl + `VecIntoIter<T>`
// adapter added to the alloc stdlib. Yields `T` by move (vs `iter()`
// yielding `&T`). Both backends supported by `VecIter<T>` work
// (commit `0c6ad117`); this fixture is the stdlib-level proof.
//
// `for x in v` (the syntactic sugar) is still rejected by typeck — it
// resolves `Iterator::next` on `iter_ty` directly and does not call
// `IntoIterator::into_iter` first. That's a separate typeck change
// tracked in KNOWN_GAPS §3.4.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let v: Vec<i32> = [1, 2, 3].to_vec();
    let mut it = v.into_iter();
    let mut sum = 0;
    loop {
        match it.next() {
            Option::Some(x) => sum = sum + x,
            Option::None => break,
        }
    }
    sum
}
