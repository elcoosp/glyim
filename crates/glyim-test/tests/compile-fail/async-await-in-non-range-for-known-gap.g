// Strict-xfail witness for GAP-0006 (plan §3.3): the async-v2 loop-await
// state machine supports `while` bodies, `for i in a..b` bodies (the
// GAP-0001 closure) and — since the GAP-0006 `loop`-shape closure —
// unbounded `loop` bodies, but a `.await` inside a `for` loop over a
// NON-RANGE iterable still cannot be re-driven across `Poll::Pending`
// (the machine has no iterator state to carry and no `next()` protocol to
// re-drive). While GAP-0006 is open, the compiler must keep rejecting this
// program with the loud T0060 hard error rather than silently miscompiling
// it.
//
// The `//@ known-gap:` header joins this fixture to
// `ledger/gaps/known/0003-await-loop-shapes.toml`; `ledger check` verifies
// the header names the gap and that the gap entry points back here. When
// the remaining async-v2 loop shapes land, this fixture FAILS with
// `known-gap GAP-0006 closed` (strict xfail): retire the witness in the
// same change — migrate the program to the pass corpus and flip the
// ledger gap entry.
//@ mode: compile-fail
//@ expect-error-code: T0060
//@ feature: LANG-ASYNC-AWAIT, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
//@ known-gap: GAP-0006
enum Poll<T> { Ready(T), Pending }
trait Future {
    type Output;
    fn poll(&mut self) -> Poll<Self::Output>;
}
async fn tick(v: i32) -> i32 { v + 1 }
async fn drain(vals: Vec<i32>) -> i32 {
    let mut acc = 0;
    for v in vals {
        acc = acc + tick(v).await; //~ ERROR inside a loop body is not yet supported
    }
    acc
}
fn main() -> i32 { 0 }
