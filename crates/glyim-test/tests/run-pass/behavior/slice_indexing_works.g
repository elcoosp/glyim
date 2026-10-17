// ANTI-REGRESSION / ANTI-MISCOMPILATION: `&arr[a..b]` slice indexing
// must work and return the correct element values.
//
// Two stacked bugs fixed here:
//   1. The `data_ptr` slot (`usize`) was filled with the first element's
//      *value* (a 4-byte `i32`) instead of its *address* (8 bytes). The
//      alloca was allocated `i32`-shaped but read back as `i64`, pulling
//      adjacent stack bytes into the high half and driving the
//      pointer arithmetic out of bounds (SEGV on `s[0]`). Fix: for an
//      array base, take a `&arr[0]` reference, `PtrToInt` to `usize`.
//   2. The range bounds (`1`, `4` in `arr[1..4]`) are unsuffixed integer
//      literals typed `i32` by default; storing them directly into
//      `usize` slots left the high half uninitialised (`store i32 2,
//      ptr ...` then `load i64` from it read garbage+bias and computed
//      a wild offset). Fix: cast each bound to `usize` through a temp
//      local when it isn't already that type.
//
// Distinct primes across three slice kinds: any residual miscompile
// shifts the exit code.
// test-mode: run-pass
// exit-code: 89
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a = [10, 20, 30, 40, 50, 60, 70, 80, 90];
    // a[2..5] = [30,40,50] -> 120; a[..3] = [10,20,30] -> 60; a[6..] = [70,80,90] -> 240
    // Sum: 120 + 60 + 240 = 420. 420 & 255 = 164.
    let s1 = &a[2..5];
    let s2 = &a[..3];
    let s3 = &a[6..];
    let sum = (s1[0] + s1[1] + s1[2])
            + (s2[0] + s2[1] + s2[2])
            + (s3[0] + s3[1] + s3[2]);
    sum & 255
}
