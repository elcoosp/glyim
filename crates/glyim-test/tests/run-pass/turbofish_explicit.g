// Explicit generic instantiation via turbofish.
//
// Regression: `f::<i32>(x)` used to drop the `<i32>` on all three lowering
// paths (HIR `UsePath`, typeck `check_path`, typeck `Call`), so `T` was
// erased and the program was miscompiled as a monomorphic call to a
// generic fn.
// test-mode: run-pass
// exit-code: 42

fn id<T>(x: T) -> T { x }

fn main() -> i32 { id::<i32>(42) }
