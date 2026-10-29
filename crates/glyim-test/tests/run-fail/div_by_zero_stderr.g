// Integer division by zero must exit 101 and emit the exact panic message.
//
// After c95bf14b the LLVM backend routes this through
// `glyim_process_abort_with_msg(ptr, i64)` → `glyim_panic`, writing
// "thread 'main' panicked: attempt to divide by zero\n" to stderr. The
// interpreter's `InterpError::DivisionByZero` Display was aligned to the
// same text, and `interpreter_runner` renders it with the same
// "thread 'main' panicked: " prefix. Both backends now match byte-for-byte,
// so this fixture no longer needs the `// needs-llvm` gate.
//
// The check-stderr matcher is LINE-anchored (TEST-1, 5b81184f).
// test-mode: run-fail
// exit-code: 101
// check-stderr: thread 'main' panicked: attempt to divide by zero
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x = 0;
    10 / x
}
