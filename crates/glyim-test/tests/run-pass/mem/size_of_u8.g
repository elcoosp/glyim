// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 { mem::size_of::<u8>() as i32 }
