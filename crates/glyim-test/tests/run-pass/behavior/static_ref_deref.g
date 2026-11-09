// `&STATIC` yields the address of a real LLVM global, so a `*p` read
// through that pointer observes the same value the initializer set.
//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-REF, LANG-SCOPE-STATIC-ITEM, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
static VALUE: i32 = 7;
fn main() -> i32 { let p = &VALUE; *p }
