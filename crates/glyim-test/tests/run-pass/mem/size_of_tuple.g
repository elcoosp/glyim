//@ mode: run-pass
//@ expect-exit: 8
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-TUPLE, COMPILER-LAYOUT-SIZE-ALIGN, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { mem::size_of::<(i32, i32)>() as i32 }
