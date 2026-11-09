// Explicit generic instantiation via turbofish.
//
// Regression: `f::<i32>(x)` used to drop the `<i32>` on all three lowering
// paths (HIR `UsePath`, typeck `check_path`, typeck `Call`), so `T` was
// erased and the program was miscompiled as a monomorphic call to a
// generic fn.
//@ mode: run-pass
//@ expect-exit: 42
//@ feature: COMPILER-MONO-MONOMORPHIZE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn id<T>(x: T) -> T { x }

fn main() -> i32 { id::<i32>(42) }
