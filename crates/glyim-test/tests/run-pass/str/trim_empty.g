// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 { if "".trim().len() == 0 { 1 } else { 0 } }
