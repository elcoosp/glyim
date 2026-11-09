// Tuple-field write through a `static mut` — exercises Field projection
// through a StaticPtr in the interpreter's write path.
//@ mode: run-pass
//@ expect-exit: 25
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-FIELD-ACCESS, LANG-TY-TUPLE, LANG-SCOPE-STATIC-ITEM, LANG-SYN-MUT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
static mut T: (i32, i32) = (10, 20);
fn main() -> i32 { unsafe { T.0 = 5; T.0 + T.1 } }
