//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: RUNTIME-FFI-ATOMIC, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let a = AtomicBool::new(true);
    let old = a.fetch_and(true, Ordering::SeqCst);
    if old { 1 } else { 0 }
}
