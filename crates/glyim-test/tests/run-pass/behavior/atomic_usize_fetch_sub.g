//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: RUNTIME-FFI-ATOMIC, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let a = AtomicUsize::new(0);
    let old = a.fetch_sub(3, Ordering::SeqCst);
    old as i32
}
