//@ mode: run-pass
//@ compile-flags: --with-stdlib
//@ expect-stdout: hi
//@ feature: STD-STD-IO, RUNTIME-FFI-IO, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() { println("hi"); }
