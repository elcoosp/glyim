//@ mode: compile-fail
//@ feature: COMPILER-TI-UNIFICATION, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
fn make<T>() -> T { }
fn main() -> i32 {
    make() //~ ERROR mismatched types
}
