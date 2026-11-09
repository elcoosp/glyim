// `1 << 2 << 3` = 32. A dropped tail would give 4.
//@ mode: run-pass
//@ expect-exit: 32
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 { 1 << 2 << 3 }
