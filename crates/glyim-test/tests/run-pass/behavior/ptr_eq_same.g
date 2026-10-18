// Raw-pointer equality of two casts of the same address must return
// `true` (lowered to a pointer-int compare + int compare).
// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x = 7;
    let p1 = &x as *const i32;
    let p2 = &x as *const i32;
    if p1 == p2 { 1 } else { 0 }
}
