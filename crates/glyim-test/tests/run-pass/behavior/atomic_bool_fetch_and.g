// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = AtomicBool::new(true);
    let old = a.fetch_and(true, Ordering::SeqCst);
    if old { 1 } else { 0 }
}
