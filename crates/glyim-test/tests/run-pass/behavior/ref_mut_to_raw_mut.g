// Implicit coercion `&mut T -> *mut T` at a `let` binding.
// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut x = 7;
    let p: *mut i32 = &mut x;
    if (p as usize) != 0 { 1 } else { 0 }
}
