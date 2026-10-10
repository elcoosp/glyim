// test-mode: run-pass
// exit-code: 8
// compile-flags: --with-stdlib
fn main() -> i32 { mem::size_of::<u64>() as i32 }
