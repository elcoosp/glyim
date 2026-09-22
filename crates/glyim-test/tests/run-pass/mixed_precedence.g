// `*` must bind tighter than `+`: 1 + 2 * 3 = 7.
// test-mode: run-pass
// exit-code: 7

fn main() -> i32 { 1 + 2 * 3 }
