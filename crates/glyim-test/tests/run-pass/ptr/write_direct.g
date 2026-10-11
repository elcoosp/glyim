// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
fn main() -> i32 { let mut x = 0; let p = &mut x as *mut i32; unsafe { *p = 7 }; x }
