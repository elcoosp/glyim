// test-mode: run-pass
// exit-code: 101
// compile-flags: --with-stdlib
fn boom() -> ! {
    extern "C" { fn glyim_process_abort() -> !; }
    unsafe { glyim_process_abort() }
}
fn main() -> i32 { boom() }
