// ANTI-MISCOMPILATION: verify `parse::<T>()` returns the *parsed value*,
// not a garbage-derived default.
//
// The `T` turbofish must reach the callee's substituted body and its
// return local must have the right layout. A regression in the mangling
// key or the method-turbofish placement used to compile fine and then
// either SIGSEGV (null-pointer store) or — if the ABI happened to align —
// return zero-initialized garbage silently.
//
// Three *distinct primes* are summed. Any sub-value that comes back as
// zero (uninit), as another sub-value (wrong body selected), or as any
// other garbage makes the exit code differ from 199.
//
// Prior behavior when this class of bug was present: the program built
// without diagnostics but returned a value that did not match the source.
// test-mode: run-pass
// exit-code: 199
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a = "61".parse::<i32>().unwrap();   // 61
    let b = "67".parse::<i32>().unwrap();   // 67
    let c = "71".parse::<i32>().unwrap();   // 71
    // 61 + 67 + 71 = 199
    a + b + c
}
