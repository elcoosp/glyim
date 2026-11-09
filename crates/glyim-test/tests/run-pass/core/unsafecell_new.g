//@ mode: run-pass
//@ expect-exit: 9
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-BUILTIN-UNSAFECELL, STD-CORE-CELL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { let c = UnsafeCell::new(9); 9 }
