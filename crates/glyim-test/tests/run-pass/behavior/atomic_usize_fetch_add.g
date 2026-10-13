// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = AtomicUsize::new(5);
    let old = a.fetch_add(3, Ordering::SeqCst);
    old as i32
}
