// Option::None.unwrap() message. §3.5.28 acceptance.
//@ mode: run-fail
//@ expect-exit: 101
//@ expect-stderr: thread 'main' panicked: called `Option::unwrap()` on a `None` value
//@ compile-flags: --with-stdlib
//@ feature: STD-CORE-OPTION, STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
fn main() -> i32 {
    let x: Option<i32> = Option::None;
    x.unwrap()
}
