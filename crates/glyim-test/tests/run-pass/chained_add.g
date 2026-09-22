// A chain of `+`: parse must build a left-associative tree, not drop operands.
// Regression: `parse_additive_expr` reset its checkpoint inside the loop, so
// `5+4+3+2+1` truncated to `5+4`.
// test-mode: run-pass
// exit-code: 15

fn main() -> i32 { 5 + 4 + 3 + 2 + 1 }
