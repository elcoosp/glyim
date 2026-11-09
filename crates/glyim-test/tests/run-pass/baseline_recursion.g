//@ mode: run-pass
//@ expect-exit: 120
//@ feature: LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn fact(n: i32) -> i32 { if n <= 1 { 1 } else { n * fact(n - 1) } }
fn main() -> i32 { fact(5) }
