//@ mode: run-pass
//@ expect-exit: 11
//@ feature: COMPILER-MONO-MONOMORPHIZE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn first<T, U>(a: T, b: U) -> T { a }

fn main() -> i32 { first(11, 22) }
