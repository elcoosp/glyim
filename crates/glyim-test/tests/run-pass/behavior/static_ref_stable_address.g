// Write through `&mut STATIC`, then read the static directly — the
// address yielded by `&STATIC` must reference the same global the
// direct `STATIC` read observes.
// test-mode: run-pass
// exit-code: 31
// compile-flags: --with-stdlib
static mut CELL: i32 = 11;
fn main() -> i32 {
    unsafe {
        let p = &mut CELL;
        *p = 31;
        CELL
    }
}
