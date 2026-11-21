// GAP-0007 strict-xfail witness (ledger/gaps/known/0007-differential-await-loop.toml):
// the class-b backend divergence the differential gate found when the
// loop-await shapes retired from xfail (2026-11-17, interpreter-only
// proof) met the native LLVM backend.
//
// The program is the GAP-0001 closure shape — `spin()` awaits inside a
// `for i in 0..3` body, the desugar normalizes the `for` into the while
// shape, and the coroutine machine re-drives the loop step across
// `Poll::Pending`. `main` drives the future by hand and exits 0 exactly
// when the accumulated sum is the expected 3, so the pinned run contract
// is a single byte: exit 0.
//
// The strict-xfail contract (plan §3.3, M1.7 backend-divergence witnesses):
//   - gam (the reference interpreter) runs it to exit 0 — the pinned run
//     holds and the witness passes;
//   - llvm (the native backend) dies by signal on this shape — the
//     declared class-b failure, witnessed debt while GAP-0007 is open;
//   - if the native backend starts running this program to exit 0, the
//     harness fails the fixture with `known-gap GAP-0007 closed` and the
//     witness must be retired in the gap-closing change.
//@ mode: run-pass
//@ expect-exit: 0
//@ feature: LANG-ASYNC-AWAIT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
//@ known-gap-backend-divergence: GAP-0007
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
            Poll::Ready(v) => {
                if v == 3 {
                    break 0;
                }
                break 1;
            }
            Poll::Pending => {}
        }
    }
}
