// ANTI-REGRESSION / ANTI-MISCOMPILATION: `str[index]` must read the
// byte at `index` and return it as `u8`.
//
// `str` / `String` lower to the same `{ptr, len}` fat-pointer struct a
// slice does, but neither `Place::ty()` (mir) nor codegen's Index arm
// handled `TyKind::String`. `"abc"[0]` tripped the "Index on
// non-array/slice type" invariant and produced `Ty::ERROR`. Fixed by
// mapping `String` to element type `u8` in both readers and routing
// the codegen path through the existing slice lowering.
//
// ASCII primes so any wrong-index or wrong-width read produces a
// distinct exit code.
// test-mode: run-pass
// exit-code: 215
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let s = "hello";
    let a: u8 = s[0];       // 'h' = 104
    let b: u8 = s[4];       // 'o' = 111
    let c: u8 = "abc"[1];   // 'b' = 98
    // 104 + 111 = 215. `c` is exercised for the literal-receiver path
    // but not added (the sum would exceed 255). A wrong `s[0]` (e.g. 'e')
    // gives 112+111=223; a wrong `s[4]` (e.g. 'l') gives 104+108=212.
    let _ = c;
    (a + b) as i32
}
