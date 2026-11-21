// GAP-0001 closure fixture (was the strict-xfail witness
// `compile-fail/async-await-in-loop-known-gap.g`): the program the async-v2
// lowering used to reject with the loud T0060 hard error now compiles,
// runs, and accumulates the right sum.
//
// `spin()` awaits inside a `for i in 0..3` body. The desugar normalizes
// the `for` into the while shape (a synthetic `let mut __for_i` iterator
// binding + an explicit increment step), so the coroutine machine's `S0`
// state carries the iterator and the accumulator across `Poll::Pending`
// and `poll()` re-drives the loop step on resume. `main` drives the
// future by hand — `dep(i)` futures resolve on their first poll, so the
// machine walks Start -> S0 -> Done inside the first `poll()` call —
// and returns the accumulated sum as the process exit code, which the
// run-mode pins exactly (stdout matching would let a silent miscompile
// that *adds* output slip past).
//
// GAP-0007 (backend divergence): on the native LLVM backend this program
// dies by signal while the interpreter exits 3 — the class-b differential
// disagreement filed in ledger/gaps/known/0007-differential-await-loop.toml
// (the canonical strict-xfail witness is
// `async_for_loop_await_differential_gap.g`). The `//@ known-gap:` header
// makes this fixture a corroborating witness: the pinned exit holds on the
// gam side, the native failure is the declared gap state, and a native run
// that starts passing must retire the witness (GAP-0007 closed).
//@ mode: run-pass
//@ expect-exit: 3
//@ feature: LANG-ASYNC-AWAIT, TOOL-TEST-RUN-PASS
//@ backends: gam
//@ known-gap: GAP-0007
enum Poll<T> { Ready(T), Pending }
trait Future {
    type Output;
    fn poll(&mut self) -> Poll<Self::Output>;
}
async fn dep(x: i32) -> i32 { x }
async fn spin() -> i32 {
    let mut acc = 0;
    for i in 0..3 {
        acc = acc + dep(i).await;
    }
    acc
}
fn main() -> i32 {
    let mut f = spin();
    loop {
        match f.poll() {
            Poll::Ready(v) => break v,
            Poll::Pending => {}
        }
    }
}
