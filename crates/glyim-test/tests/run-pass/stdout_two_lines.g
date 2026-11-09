//@ mode: run-pass
//@ compile-flags: --with-stdlib
//@ expect-stdout: alpha
//@ feature: STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() {
    println("alpha");
    println("beta");
}
