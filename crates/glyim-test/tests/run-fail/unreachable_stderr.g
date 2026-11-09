// unreachable!() message. §3.5.28 acceptance.
//@ mode: run-fail
//@ expect-exit: 101
//@ expect-stderr: thread 'main' panicked: internal error: entered unreachable code
//@ compile-flags: --with-stdlib
//@ feature: STD-CORE-PANIC, STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
fn main() -> i32 {
    unreachable!()
}
