// `for x in a.iter()` — same index-walk lowering as for_loop_over_slice.
// `iter()` (builtin FnDefId 9162) returns `&[T]`; the for-loop then
// index-walks it. Closes the interp slice-iter gap on `a.iter()` too.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = [1, 2, 3];
    let mut s = 0;
    for x in a.iter() {
        s = s + *x;
    }
    s
}
