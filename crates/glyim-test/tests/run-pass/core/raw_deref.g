// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
fn main() -> i32 { let x = 7; let p = &x as *const i32; unsafe { *p } }
