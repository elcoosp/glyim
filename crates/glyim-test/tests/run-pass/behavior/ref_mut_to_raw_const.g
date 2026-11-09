// Implicit coercion `&mut T -> *const T` (a *const promises only shared
// access, so the mut direction is a one-way `Mut -> Not`).
//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-REF-MUT, LANG-SYN-MUT, LANG-TY-REF, LANG-TY-RAW-PTR, LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut x = 7;
    let p: *const i32 = &mut x;
    if (p as usize) != 0 { 1 } else { 0 }
}
