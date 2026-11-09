//@ mode: compile-fail
//@ feature: COMPILER-FRONT-DEFTABLE, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
fn main() -> i32 {
    undefined_symbol //~ ERROR unresolved name
}
