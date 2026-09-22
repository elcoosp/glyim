// test-mode: run-pass
// exit-code: 18

fn a(x: i32) -> i32 { x + 1 }
fn b(x: i32) -> i32 { a(x) + a(x) }
fn c(x: i32) -> i32 { b(x) + a(x) }

fn main() -> i32 { c(5) }
