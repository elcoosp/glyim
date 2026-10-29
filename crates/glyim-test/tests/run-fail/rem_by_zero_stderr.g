// Integer remainder by zero must exit 101 and emit the exact panic message.
//
// Companion to div_by_zero_stderr.g. Exercises both backends:
//   - LLVM: the rem-by-zero abort site routes through
//     `glyim_process_abort_with_msg` (c95bf14b).
//   - Interp: `InterpError::RemainderByZero` (new variant, Display matches
//     the LLVM message).
// check-stderr matcher is line-anchored (TEST-1).
// test-mode: run-fail
// exit-code: 101
// check-stderr: thread 'main' panicked: attempt to calculate the remainder with a divisor of zero
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x = 0;
    10 % x
}
