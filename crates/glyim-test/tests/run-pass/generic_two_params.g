// test-mode: run-pass
// exit-code: 11

fn first<T, U>(a: T, b: U) -> T { a }

fn main() -> i32 { first(11, 22) }
