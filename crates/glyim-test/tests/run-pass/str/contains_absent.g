// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 { if "abc".contains("x") { 1 } else { 0 } }
