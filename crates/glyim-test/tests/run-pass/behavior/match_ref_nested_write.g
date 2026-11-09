// Regression: `ref mut` write on a nested Option payload; guards the
// temp-copy aliasing bug on a non-trivial scrutinee.
//@ mode: run-pass
//@ expect-exit: 14
//@ compile-flags: --with-stdlib
//@ needs-llvm
//@ feature: LANG-MATCH-GUARDS, LANG-TY-REF, LANG-SYN-MUT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut inner = Option::Some(7);
    match inner {
        Option::Some(ref mut v) => { *v = *v * 2; }
        Option::None => {}
    }
    inner.unwrap()
}
