// TEST-7 regression: a program that prints *before* panicking must keep
// its output in the interpreter path. Under the old shape the `Err` arm of
// `interpret_bodies` returned `stdout: []`, so `before` was silently
// dropped. The fix hoists the buffer reads above the match, so both arms
// see whatever the program wrote.
//@ mode: run-fail
//@ expect-exit: 101
//@ expect-stdout: before
//@ compile-flags: --with-stdlib
//@ feature: STD-CORE-PANIC, RUNTIME-FFI-PANIC, STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
fn main() -> i32 {
    println("before");
    assert!(false, "boom");
    0
}
