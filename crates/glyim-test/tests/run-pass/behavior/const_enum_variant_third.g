// Third-of-three variant, so the emitted discriminant must be exactly 2
// (not just "non-zero").
//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-ENUM, LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
enum C { A, B, D }
const K: C = C::D;
fn main() -> i32 { match K { C::A => 1, C::B => 2, C::D => 3 } }
