// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
fn wr(d: *mut i32, s: i32) { unsafe { *d = s } }
fn main() -> i32 { let mut x = 0; let p = &mut x as *mut i32; unsafe { wr(p, 7) }; x }
