// The stdlib's actual shape: a unit struct with inherent methods, used
// through a `const` of that type.
//@ mode: run-pass
//@ expect-exit: 29
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-STRUCT, LANG-SYN-TUPLE-STRUCT, LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct Counter;
impl Counter {
    fn value(&self) -> i32 { 29 }
}
const C: Counter = Counter;
fn main() -> i32 { C.value() }
