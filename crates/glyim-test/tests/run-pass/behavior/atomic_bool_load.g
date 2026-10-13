// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = AtomicBool::new(true);
    if a.load(Ordering::SeqCst) { 1 } else { 0 }
}
