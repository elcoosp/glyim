// `AtomicBool::compare_exchange`: exchange `false` -> `true` succeeds, so
// `load()` observes the new value.
// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = AtomicBool::new(false);
    let r = a.compare_exchange(false, true);
    if r.is_ok() { if a.load() { 1 } else { 0 } } else { -1 }
}
