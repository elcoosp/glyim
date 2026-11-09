//@ mode: compile-pass
//@ feature: COMPILER-MONO-MONOMORPHIZE, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
fn identity<T>(x: T) -> T { x }
fn main() -> i32 { identity(42) }
