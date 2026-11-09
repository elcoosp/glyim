// `static mut` write followed by a read sees the write (the address is
// a real mutable LLVM global, not a stack temp).
//@ mode: run-pass
//@ expect-exit: 10
//@ compile-flags: --with-stdlib
//@ feature: LANG-SCOPE-STATIC-ITEM, LANG-SYN-MUT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
static mut COUNTER: i32 = 7;
fn main() -> i32 { unsafe { COUNTER = 10; COUNTER } }
