//@ mode: run-pass
//@ expect-exit: 101
//@ compile-flags: --with-stdlib
//@ feature: RUNTIME-FFI-PROCESS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn boom() -> ! {
    extern "C" { fn glyim_process_abort() -> !; }
    unsafe { glyim_process_abort() }
}
fn main() -> i32 { boom() }
