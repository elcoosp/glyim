//@ mode: compile-pass
// V16-T03: panic! macro prints message and aborts
//@ feature: STD-CORE-PANIC, RUNTIME-FFI-PANIC, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    panic!("this is a panic message");
}
