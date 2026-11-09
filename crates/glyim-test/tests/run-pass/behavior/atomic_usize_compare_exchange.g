// `AtomicUsize::compare_exchange` success path: exchange succeeds iff the
// current value equals `expected`; the result is `Result::Ok(prev)`. After
// the successful exchange the stored value is the new one.
//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ feature: RUNTIME-FFI-ATOMIC, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let a = AtomicUsize::new(5);
    let r = a.compare_exchange(5, 7);
    if r.is_ok() { a.load() as i32 } else { 0 }
}
