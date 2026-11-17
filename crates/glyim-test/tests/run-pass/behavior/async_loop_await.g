// GAP-0006 `loop`-shape closure fixture (was the strict-xfail witness
// `compile-fail/async-await-in-unbounded-loop-known-gap.g`): the program the
// async-v2 lowering used to reject with the loud T0060 hard error now
// compiles, runs, and accumulates the right sum.
//
// `spin()` awaits inside an unbounded `loop` body and exits through a
// depth-0 `break acc`. The desugar normalizes the `loop` into the
// `while true` shape (a fresh literal-`true` condition; the body carried
// over untouched), so the coroutine machine's `S0` state carries the
// accumulator and the counter across `Poll::Pending`, `poll()` re-drives
// the loop step on resume, and the `break` translation turns the loop exit
// into the machine's `Done { result: acc }` transition. `main` drives the
// future by hand — `dep(i)` futures resolve on their first poll, so the
// machine walks Start -> S0 -> Done inside the first `poll()` call — and
// returns the accumulated sum as the process exit code, which the run-mode
// pins exactly (stdout matching would let a silent miscompile that *adds*
// output slip past).
//@ mode: run-pass
//@ expect-exit: 11
//@ feature: LANG-ASYNC-AWAIT, TOOL-TEST-RUN-PASS
//@ backends: gam
enum Poll<T> { Ready(T), Pending }
trait Future {
    type Output;
    fn poll(&mut self) -> Poll<Self::Output>;
}
async fn tick(v: i32) -> i32 { v + 1 }
async fn spin() -> i32 {
    let mut acc = 0;
    loop {
        acc = acc + tick(0).await;
        if acc > 10 {
            break acc;
        }
    }
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
