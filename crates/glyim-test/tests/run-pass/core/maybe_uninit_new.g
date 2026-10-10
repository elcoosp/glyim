// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 { let m = mem::MaybeUninit::new(5); let v = unsafe { m.assume_init() }; v }
