// A chain of `+`: parse must build a left-associative tree, not drop operands.
// Regression: `parse_additive_expr` reset its checkpoint inside the loop, so
// `5+4+3+2+1` truncated to `5+4`.
//@ mode: run-pass
//@ expect-exit: 15
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 { 5 + 4 + 3 + 2 + 1 }
