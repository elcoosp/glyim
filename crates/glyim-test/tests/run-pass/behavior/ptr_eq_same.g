// Raw-pointer equality of two casts of the same address must return
// `true` (lowered to a pointer-int compare + int compare).
//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-RAW-PTR, STD-CORE-PTR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let x = 7;
    let p1 = &x as *const i32;
    let p2 = &x as *const i32;
    if p1 == p2 { 1 } else { 0 }
}
