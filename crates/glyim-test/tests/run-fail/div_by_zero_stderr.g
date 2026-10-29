// TEST-1 first real user + §3.5.27 verification: integer division by zero
// must exit 101 and emit the exact panic message to stderr.
//
// Before c95bf14b the abort site was `call void @glyim_process_abort()` with
// zero args, which exited silently (exit 101, empty stderr). After the fix
// it routes through `glyim_process_abort_with_msg(ptr, i64)` → `glyim_panic`,
// which writes "thread 'main' panicked: <msg>\n" to stderr before exiting.
//
// The check-stderr matcher is LINE-anchored (TEST-1, 5b81184f): the expected
// text must match a full output line exactly.
//
// `needs-llvm`: asserts the LLVM backend's abort message. The interpreter's
// `glyim_process_abort` arm returns "program aborted"
// (crates/glyim-mir-interp/src/lib.rs:1593) — a divergence to unify in a
// separate fix. On a non-real-LLVM host this fixture is Ignored.
// test-mode: run-fail
// exit-code: 101
// needs-llvm
// check-stderr: thread 'main' panicked: attempt to divide by zero
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x = 0;
    10 / x
}
