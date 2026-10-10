// test-mode: run-pass
// exit-code: 4
// compile-flags: --with-stdlib
fn main() -> i32 { let a = [1, 2, 3, 4]; let s = &a[0..4]; s.len() as i32 }
