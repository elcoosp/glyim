// `AtomicBool::compare_exchange`: exchange `false` -> `true` succeeds, so
// `load()` observes the new value.
//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: RUNTIME-FFI-ATOMIC, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let a = AtomicBool::new(false);
    let r = a.compare_exchange(false, true);
    if r.is_ok() { if a.load() { 1 } else { 0 } } else { -1 }
}
