// `x.clone()` on a Copy primitive must yield the same value (P1-C1).
// Before this fix the compiler resolved `clone` to a sentinel FnDefId and
// lowering replaced the call with a zeroing `MirConstKind::Error` constant
// even on live paths — this fixture produced 0 instead of 5.
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x: i32 = 5;
    let y = x.clone();
    y
}
