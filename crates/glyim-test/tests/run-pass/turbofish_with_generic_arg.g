// Turbofish combined with a generic-typed argument: both the explicit and
// the inferred source of the substitution must agree.
//@ mode: run-pass
//@ expect-exit: 7
//@ feature: COMPILER-MONO-MONOMORPHIZE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn first<T>(a: T, b: T) -> T { a }

fn main() -> i32 { first::<i32>(7, 8) }
