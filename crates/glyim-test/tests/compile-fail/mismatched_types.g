//@ mode: compile-fail
//@ feature: COMPILER-TI-UNIFICATION, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
fn main() -> i32 {
    let x: i32 = 1;
    let y: bool = true;
    x + y //~ ERROR mismatched types
}
