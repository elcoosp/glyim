// `AtomicUsize::compare_exchange` success path: exchange succeeds iff the
// current value equals `expected`; the result is `Result::Ok(prev)`. After
// the successful exchange the stored value is the new one.
// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = AtomicUsize::new(5);
    let r = a.compare_exchange(5, 7);
    if r.is_ok() { a.load() as i32 } else { 0 }
}
