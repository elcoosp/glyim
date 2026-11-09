//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ feature: LANG-CLOSURE-EXPR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let f = || 7;
    f()
}
