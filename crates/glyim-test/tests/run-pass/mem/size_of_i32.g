// test-mode: run-pass
// exit-code: 4
// compile-flags: --with-stdlib
fn main() -> i32 { mem::size_of::<i32>() as i32 }
