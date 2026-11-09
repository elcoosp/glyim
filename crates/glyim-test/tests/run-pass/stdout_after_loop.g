//@ mode: run-pass
//@ compile-flags: --with-stdlib
//@ expect-stdout: done
//@ feature: STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() {
    let mut i = 0;
    while i < 3 {
        i = i + 1;
    }
    if i == 3 { println("done"); }
}
