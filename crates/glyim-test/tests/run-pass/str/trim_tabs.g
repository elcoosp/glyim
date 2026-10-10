// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 { if "\t\tx\t".trim().len() == 1 { 1 } else { 0 } }
