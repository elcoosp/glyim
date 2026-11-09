//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: RUNTIME-FFI-ATOMIC, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let a = AtomicBool::new(true);
    a.store(false, Ordering::SeqCst);
    if a.load(Ordering::SeqCst) { 1 } else { 0 }
}
