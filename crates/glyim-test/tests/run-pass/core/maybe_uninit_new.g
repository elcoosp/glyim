//@ mode: run-pass
//@ expect-exit: 5
//@ compile-flags: --with-stdlib
//@ feature: STD-CORE-MEM, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { let m = mem::MaybeUninit::new(5); let v = unsafe { m.assume_init() }; v }
