// Sanity: a `static mut` read without a preceding write returns the
// initializer value.
// test-mode: run-pass
// exit-code: 19
// compile-flags: --with-stdlib
static mut SEED: i32 = 19;
fn main() -> i32 { unsafe { SEED } }
