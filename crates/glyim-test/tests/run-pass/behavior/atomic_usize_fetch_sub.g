// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = AtomicUsize::new(0);
    let old = a.fetch_sub(3, Ordering::SeqCst);
    old as i32
}
