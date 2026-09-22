// test-mode: run-pass
// exit-code: 42
fn id<T>(x: T) -> T { x }
fn main() -> i32 { id(42) }
