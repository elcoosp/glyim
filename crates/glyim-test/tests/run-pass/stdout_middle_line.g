//@ mode: run-pass
//@ compile-flags: --with-stdlib
//@ expect-stdout: line two
//@ feature: STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() {
    println("line one");
    println("line two");
    println("line three");
}
