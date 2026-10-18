// Implicit coercion `&mut T -> *const T` (a *const promises only shared
// access, so the mut direction is a one-way `Mut -> Not`).
// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut x = 7;
    let p: *const i32 = &mut x;
    if (p as usize) != 0 { 1 } else { 0 }
}
