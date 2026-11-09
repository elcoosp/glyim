// Write through `&mut STATIC`, then read the static directly — the
// address yielded by `&STATIC` must reference the same global the
// direct `STATIC` read observes.
//@ mode: run-pass
//@ expect-exit: 31
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-REF, LANG-SCOPE-STATIC-ITEM, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
static mut CELL: i32 = 11;
fn main() -> i32 {
    unsafe {
        let p = &mut CELL;
        *p = 31;
        CELL
    }
}
