//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: COMPILER-LAYOUT-SIZE-ALIGN, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { mem::align_of::<u8>() as i32 }
