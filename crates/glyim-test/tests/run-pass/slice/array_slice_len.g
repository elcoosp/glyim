// test-mode: run-pass
// exit-code: 2
// compile-flags: --with-stdlib
fn main() -> i32 { let a = [1, 2, 3, 4]; let s = &a[1..3]; s.len() as i32 }
