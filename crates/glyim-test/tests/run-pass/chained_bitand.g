//@ mode: run-pass
//@ expect-exit: 3
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 { 7 & 3 }
