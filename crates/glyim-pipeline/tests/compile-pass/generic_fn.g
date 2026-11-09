//@ mode: compile-pass
//@ compile-flags: --backend mock
//@ feature: COMPILER-MONO-MONOMORPHIZE, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn id<T>(x: T) -> T {
    x
}

fn main() -> i32 {
    let a: i32 = id(42);
    let b: bool = id(true);
    a
}
