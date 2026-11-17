// Strict-xfail witness for GAP-0006 (plan §3.3): the async-v2 loop-await
// state machine supports `while` bodies and — since the GAP-0001 closure —
// `for i in a..b` bodies, but a `.await` inside a `loop { .. }` body still
// cannot be re-driven across `Poll::Pending` (the machine has no resume
// point for the unbounded dispatch arm). While GAP-0006 is open, the
// compiler must keep rejecting this program with the loud T0060 hard
// error rather than silently miscompiling it.
//
// The `//@ known-gap:` header joins this fixture to
// `ledger/gaps/known/0003-await-loop-shapes.toml`; `ledger check` verifies
// the header names the gap and that the gap entry points back here. When
// the remaining async-v2 loop shapes land, this fixture FAILS with
// `known-gap GAP-0006 closed` (strict xfail): retire the witness in the
// same change — migrate the program to the pass corpus and flip the
// ledger gap entry.
//@ mode: compile-fail
//@ feature: LANG-ASYNC-AWAIT, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
//@ known-gap: GAP-0006
enum Poll<T> { Ready(T), Pending }
trait Future {
    type Output;
    fn poll(&mut self) -> Poll<Self::Output>;
}
async fn tick(v: i32) -> i32 { v + 1 }
async fn spin() -> i32 {
    let mut acc = 0;
    loop {
        acc = acc + tick(0).await; //~ ERROR inside a loop body is not yet supported
        if acc > 10 {
            break acc;
        }
    }
}
fn main() -> i32 { 0 }
