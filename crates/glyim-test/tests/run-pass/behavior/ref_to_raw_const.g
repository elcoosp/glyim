// Implicit coercion `&T -> *const T` at a `let` binding.
// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x = 7;
    let p: *const i32 = &x;
    if (p as usize) != 0 { 1 } else { 0 }
}
