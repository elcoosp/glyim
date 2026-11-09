// By-value read of a `static` item whose initializer const-evaluates.
//@ mode: run-pass
//@ expect-exit: 71
//@ compile-flags: --with-stdlib
//@ feature: LANG-SCOPE-STATIC-ITEM, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
static VALUE: i32 = 71;
fn main() -> i32 { VALUE }
