// M1.6 strict-xfail witness for GAP-0001 (plan §3.3).
//
// `.await` inside a `for` loop is one of the shapes the async-v2
// state-machine lowering does not support yet (the single-poll desugar
// would spin the in-loop `Pending` into a `loop {}` hang, so lowering
// refuses with the loud T0060 diagnostic instead — KNOWN_GAPS §1). While
// GAP-0001 is open, the compiler must keep rejecting this program.
//
// The `//@ known-gap:` header joins this fixture to
// `ledger/gaps/known/0001-known-gaps-seed.toml`; `ledger check` verifies
// the header names the gap and that the gap entry points back here. When
// the async-v2 lowering lands, this fixture FAILS with
// `known-gap GAP-0001 closed` (strict xfail): retire the witness in the
// same change — migrate the program to the pass corpus and flip the
// ledger gap entry.
//@ mode: compile-fail
//@ feature: LANG-ASYNC-AWAIT, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
//@ known-gap: GAP-0001
enum Poll<T> { Ready(T), Pending }
trait Future {
    type Output;
    fn poll(&mut self) -> Poll<Self::Output>;
}
async fn tick(v: i32) -> i32 { v + 1 }
async fn spin() -> i32 {
    let mut acc = 0;
    for i in 0..3 {
        acc = acc + tick(i).await; //~ ERROR inside a loop body is not yet supported
    }
    acc
}
fn main() -> i32 { 0 }
