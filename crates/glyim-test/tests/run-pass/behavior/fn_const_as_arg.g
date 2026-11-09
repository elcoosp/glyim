// Fn-pointer-valued constant passed through to a `fn(..)` parameter,
// then called through the FnPtr path inside the callee.
//@ mode: run-pass
//@ expect-exit: 42
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn double(x: i32) -> i32 { x * 2 }
const D: fn(i32) -> i32 = double;
fn apply(f: fn(i32) -> i32, v: i32) -> i32 { f(v) }
fn main() -> i32 { apply(D, 21) }
