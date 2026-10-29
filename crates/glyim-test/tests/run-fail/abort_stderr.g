// Explicit `abort()` must exit 101 and emit the same message on both
// backends.
//
// Companion to §3.5.27 (abort_with_msg unification) and §3.5.28
// (message-loss in divergent paths). Previously the zero-arg
// glyim_process_abort() exited 101 silently on LLVM while the interp
// said "thread 'main' panicked: program aborted". Now both agree.
//
// Uses `process.abort` (the stdlib wrapper) so the call path is the same
// one a user would write. `abort_stops.g` (run-pass, exit-code: 101)
// asserts the exit code on both backends; this fixture asserts the text.
// test-mode: run-fail
// exit-code: 101
// check-stderr: thread 'main' panicked: program aborted
// compile-flags: --with-stdlib
fn main() -> i32 {
    unsafe { glyim_process_abort() }
}
