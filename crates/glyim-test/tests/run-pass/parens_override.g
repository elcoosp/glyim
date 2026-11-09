//@ mode: run-pass
//@ expect-exit: 9
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 { (1 + 2) * 3 }
