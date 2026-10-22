// `for x in &v` where `v: Vec<T>` — supported via `as_slice` (FnDefId
// 9144) then the proven slice index-walk. Item type is `&T` (matching
// `impl Iterator for &[T]`).
//
// Before this: `for x in &v` was a **rejected** path with an
// `as_slice()` hint — the trait path reached `impl Iterator for &[T]`,
// which returns `&T` into its own transient frame, and the lowering
// passed `&mut &Vec<T>` (3-word) to `<&[T]>::next` (2-word). LLVM
// segfaulted; the interpreter panicked `deref of Ref from dead frame`.
// The fix materializes `&[T]` via `Vec::as_slice` first, then reuses
// the slice index-walk (which keeps the reference target in the
// caller's frame). Both backends now agree.
//
// By-value `for x in v` still needs a real `IntoIterator for Vec<T>`
// yielding `T` by move (see KNOWN_GAPS §3.4) and remains rejected with
// the same hint.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let v: Vec<i32> = [1, 2, 3].to_vec();
    let mut s = 0;
    for x in &v {
        s = s + *x;
    }
    s
}
