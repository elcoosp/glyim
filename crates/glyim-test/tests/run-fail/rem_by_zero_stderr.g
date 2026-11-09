// Integer remainder by zero must exit 101 and emit the exact panic message.
//
// Companion to div_by_zero_stderr.g. Exercises both backends:
//   - LLVM: the rem-by-zero abort site routes through
//     `glyim_process_abort_with_msg` (c95bf14b).
//   - Interp: `InterpError::RemainderByZero` (new variant, Display matches
//     the LLVM message).
// check-stderr matcher is line-anchored (TEST-1).
//@ mode: run-fail
//@ expect-exit: 101
//@ expect-stderr: thread 'main' panicked: attempt to calculate the remainder with a divisor of zero
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-OPERATORS, STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
fn main() -> i32 {
    let x = 0;
    10 % x
}
