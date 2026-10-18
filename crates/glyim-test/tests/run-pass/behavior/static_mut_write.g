// `static mut` write followed by a read sees the write (the address is
// a real mutable LLVM global, not a stack temp).
// test-mode: run-pass
// exit-code: 10
// compile-flags: --with-stdlib
static mut COUNTER: i32 = 7;
fn main() -> i32 { unsafe { COUNTER = 10; COUNTER } }
