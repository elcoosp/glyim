//@ mode: run-pass
//@ compile-flags: --with-stdlib
//@ expect-stdout: positive
//@ feature: STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() {
    let n = 5;
    if n > 0 {
        println("positive");
    } else {
        println("non-positive");
    }
}
