// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = AtomicBool::new(true);
    a.store(false, Ordering::SeqCst);
    if a.load(Ordering::SeqCst) { 1 } else { 0 }
}
