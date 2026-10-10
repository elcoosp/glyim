// test-mode: run-pass
// exit-code: 9
// compile-flags: --with-stdlib
fn main() -> i32 { let c = UnsafeCell::new(9); 9 }
