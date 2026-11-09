//@ mode: run-pass
//@ expect-exit: 18
//@ feature: LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn a(x: i32) -> i32 { x + 1 }
fn b(x: i32) -> i32 { a(x) + a(x) }
fn c(x: i32) -> i32 { b(x) + a(x) }

fn main() -> i32 { c(5) }
