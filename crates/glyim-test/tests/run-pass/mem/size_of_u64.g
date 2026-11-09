//@ mode: run-pass
//@ expect-exit: 8
//@ compile-flags: --with-stdlib
//@ feature: COMPILER-LAYOUT-SIZE-ALIGN, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { mem::size_of::<u64>() as i32 }
