// Deref + Field write through a `&mut` borrowed from a static mut.
//@ mode: run-pass
//@ expect-exit: 31
//@ compile-flags: --with-stdlib
//@ feature: LANG-SCOPE-STATIC-ITEM, LANG-SYN-MUT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct S { v: i32 }
static mut S_OBJ: S = S { v: 5 };
fn main() -> i32 {
    unsafe {
        let p: &mut S = &mut S_OBJ;
        (*p).v = 31;
        S_OBJ.v
    }
}
