// `for x in &slice`: iterate a `&[T]` by reference. Exercises the
// `IntoIterator`/`Iterator for &[T]` path plus the deferred `SizeOf`
// element stride in the dynamic slice range.
//@ needs-llvm
//@ mode: run-pass
//@ expect-exit: 6
//@ compile-flags: --with-stdlib
//@ feature: LANG-FOR-SLICE-ITER, LANG-TY-SLICE, STD-CORE-SLICE, LANG-TY-REF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let a = [1, 2, 3];
    let s: &[i32] = &a;
    let mut n = 0;
    for x in s { n = n + *x; }
    n
}
