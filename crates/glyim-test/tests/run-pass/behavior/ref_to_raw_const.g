// Implicit coercion `&T -> *const T` at a `let` binding.
//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-REF, LANG-TY-RAW-PTR, LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let x = 7;
    let p: *const i32 = &x;
    if (p as usize) != 0 { 1 } else { 0 }
}
