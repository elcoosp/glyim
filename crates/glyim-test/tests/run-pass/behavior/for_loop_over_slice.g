// `for x in v.as_slice()` — index-walk lowering (see KNOWN_GAPS §7).
// Previously the trait path routed through `impl Iterator for &[T]`,
// whose `next` returns `&T` into its own transient frame — that is
// unrepresentable in the interpreter's `Ref { frame, local }` model
// and panicked with "deref of Ref from dead frame 1". The index-walk
// keeps the reference target in the caller's frame and works on both
// backends.
// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    let v: Vec<i32> = [1, 2].to_vec();
    let mut s = 0;
    for x in v.as_slice() {
        s = s + *x;
    }
    s
}
