// assert!(false, "boom") must emit the assertion message. §3.5.29 fix:
// user-macro dispatch now prefers the .g assert! from panic.g over the
// Rust-side builtin. The .g macro routes through panic_any_str → glyim_panic.
// test-mode: run-fail
// exit-code: 101
// check-stderr: thread 'main' panicked: assertion failed: boom
// compile-flags: --with-stdlib
fn main() -> i32 {
    assert!(false, "boom");
    0
}
