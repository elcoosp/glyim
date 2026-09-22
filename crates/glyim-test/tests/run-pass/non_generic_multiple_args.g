// Non-generic fn with multiple primitive args must not be treated as generic.
// Regression: the Call arm built FnDef substs from the formal param list.
// test-mode: run-pass
// exit-code: 9

fn combine(a: i32, b: i32) -> i32 { a + b }

fn main() -> i32 { combine(4, 5) }
