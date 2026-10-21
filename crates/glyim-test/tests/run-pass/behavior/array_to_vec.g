// `[T; N].to_vec()` — an *array* receiver, not a slice. Method resolution
// finds the slice builtin but does not insert the array-to-slice unsize
// coercion, so codegen handled only the `{ptr, len}` fat-pointer case and
// emitted an undefined `__glyim_fn_<builtin>` symbol. Codegen now spills the
// array and passes its address plus the static length.
// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = [1, 2, 3];
    let v = a.to_vec();
    v.len() as i32
}
