// Multi-file: `mod helper;` pulls in `helper.g` next to this file.
// test-mode: run-pass
// exit-code: 42
mod helper;
fn main() -> i32 { helper::value() }
