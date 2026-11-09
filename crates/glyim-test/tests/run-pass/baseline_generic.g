//@ mode: run-pass
//@ expect-exit: 42
//@ feature: TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn id<T>(x: T) -> T { x }
fn main() -> i32 { id(42) }
