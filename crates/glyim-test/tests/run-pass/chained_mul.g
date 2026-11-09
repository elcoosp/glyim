//@ mode: run-pass
//@ expect-exit: 24
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 { 2 * 3 * 4 }
