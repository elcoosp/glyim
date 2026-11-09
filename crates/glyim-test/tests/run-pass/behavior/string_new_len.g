//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-STRING, STD-ALLOC-STRING, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    String::new().len() as i32
}
