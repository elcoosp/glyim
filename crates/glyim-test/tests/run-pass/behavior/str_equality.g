// ANTI-REGRESSION / ANTI-MISCOMPILATION: `==` and `!=` on `&str` /
// `String` must work.
//
// Typeck permitted `==` on any unifiable pair, but codegen's `Eq`/`Ne`
// arms only handled int/float and errored on string operands with
// "eq: mismatched types". The fix routes string-shaped operands (LLVM
// `{ptr, len}` structs) through the runtime helper `glyim_str_eq`.
// test-mode: run-pass
// exit-code: 47
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a: i32 = if "abc" == "abc" { 11 } else { 0 };
    let b: i32 = if "abc" == "xyz" { 0 } else { 13 };
    let c: i32 = if "abc" != "xyz" { 17 } else { 0 };
    let d: i32 = if "abc" != "abc" { 0 } else { 6 };
    a + b + c + d
}
