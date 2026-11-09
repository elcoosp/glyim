// assert!(false) single-arg (stringify! path) — companion fixture.
//@ mode: run-fail
//@ expect-exit: 101
//@ expect-stderr: thread 'main' panicked: assertion failed: false
//@ compile-flags: --with-stdlib
//@ feature: STD-CORE-PANIC, STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
fn main() -> i32 {
    assert!(false);
    0
}
