//@ mode: compile-pass
// V18-T01: Write to stdout via println!
//@ feature: STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    println!("hello, world!");
    println!("number: {}", 42);
}
