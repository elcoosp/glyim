// test-mode: compile-pass
fn identity<T>(x: T) -> T { x }
fn main() -> i32 { identity(42) }
