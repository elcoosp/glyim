//@ mode: run-pass
//@ expect-exit: 4
//@ compile-flags: --with-stdlib
//@ feature: COMPILER-LAYOUT-SIZE-ALIGN, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { mem::size_of::<i32>() as i32 }
