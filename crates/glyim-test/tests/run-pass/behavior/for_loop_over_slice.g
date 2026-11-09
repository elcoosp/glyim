// `for x in v.as_slice()` — index-walk lowering (see KNOWN_GAPS §7).
// Previously the trait path routed through `impl Iterator for &[T]`,
// whose `next` returns `&T` into its own transient frame — that is
// unrepresentable in the interpreter's `Ref { frame, local }` model
// and panicked with "deref of Ref from dead frame 1". The index-walk
// keeps the reference target in the caller's frame and works on both
// backends.
//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: LANG-FOR-SLICE-ITER, LANG-TY-SLICE, STD-CORE-SLICE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let v: Vec<i32> = [1, 2].to_vec();
    let mut s = 0;
    for x in v.as_slice() {
        s = s + *x;
    }
    s
}
