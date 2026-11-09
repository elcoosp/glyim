// Struct-typed constant read: exercises the structural Adt LLVM type
// so `Aggregate` const lowering can insert field 1 without out-of-range.
//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-STRUCT, LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct Pair { a: i32, b: i32 }
const P: Pair = Pair { a: 3, b: 4 };
fn main() -> i32 { P.a + P.b }
