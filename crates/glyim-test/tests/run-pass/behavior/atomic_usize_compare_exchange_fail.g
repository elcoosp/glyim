// `AtomicUsize::compare_exchange` failure path: `expected` does not match the
// current value, so the exchange does not happen; the result is
// `Result::Err(prev)` and the stored value is unchanged.
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = AtomicUsize::new(5);
    let r = a.compare_exchange(9, 7);
    if r.is_err() { a.load() as i32 } else { 0 }
}
