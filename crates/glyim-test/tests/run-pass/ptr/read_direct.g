// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 { let x = 5; let p = &x as *const i32; unsafe { *p } }
