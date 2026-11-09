//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { (40 + 2) }
