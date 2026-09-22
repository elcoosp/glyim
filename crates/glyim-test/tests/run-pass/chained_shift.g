// `1 << 2 << 3` = 32. A dropped tail would give 4.
// test-mode: run-pass
// exit-code: 32

fn main() -> i32 { 1 << 2 << 3 }
