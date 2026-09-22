// Turbofish combined with a generic-typed argument: both the explicit and
// the inferred source of the substitution must agree.
// test-mode: run-pass
// exit-code: 7

fn first<T>(a: T, b: T) -> T { a }

fn main() -> i32 { first::<i32>(7, 8) }
