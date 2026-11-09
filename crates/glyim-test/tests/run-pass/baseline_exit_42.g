//@ mode: run-pass
//@ expect-exit: 42
//@ feature: RUNTIME-FFI-PROCESS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 { 42 }
