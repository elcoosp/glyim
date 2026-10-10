// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 { if "   abc".trim().len() == 3 { 1 } else { 0 } }
