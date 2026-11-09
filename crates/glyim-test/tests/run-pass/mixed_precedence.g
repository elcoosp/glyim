// `*` must bind tighter than `+`: 1 + 2 * 3 = 7.
//@ mode: run-pass
//@ expect-exit: 7
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 { 1 + 2 * 3 }
