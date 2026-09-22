// Multi-file with nesting: main includes `outer`, which includes `inner`.
// test-mode: run-pass
// exit-code: 7
mod outer;
fn main() -> i32 { outer::via_inner() }
