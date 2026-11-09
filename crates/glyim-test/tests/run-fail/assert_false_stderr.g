// assert!(false, "boom") must emit the assertion message. §3.5.29 fix:
// user-macro dispatch now prefers the .g assert! from panic.g over the
// Rust-side builtin. The .g macro routes through panic_any_str → glyim_panic.
//@ mode: run-fail
//@ expect-exit: 101
//@ expect-stderr: thread 'main' panicked: assertion failed: boom
//@ compile-flags: --with-stdlib
//@ feature: STD-CORE-PANIC, STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
fn main() -> i32 {
    assert!(false, "boom");
    0
}
