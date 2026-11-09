//@ mode: run-pass
//@ compile-flags: --with-stdlib
//@ expect-exit: 10
//@ feature: LANG-TY-SLICE, STD-CORE-SLICE, STD-CORE-ITER, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let s: &[i32] = &[10, 20, 30];
    let mut it = s.iter();
    match it.next() { Some(x) => *x, None => 0 }
}
