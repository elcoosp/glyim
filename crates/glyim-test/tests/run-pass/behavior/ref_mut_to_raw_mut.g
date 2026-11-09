// Implicit coercion `&mut T -> *mut T` at a `let` binding.
//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-REF-MUT, LANG-SYN-MUT, LANG-TY-REF, LANG-TY-RAW-PTR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut x = 7;
    let p: *mut i32 = &mut x;
    if (p as usize) != 0 { 1 } else { 0 }
}
