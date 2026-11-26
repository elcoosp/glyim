//@ mode: compile-fail
//@ expect-error-code: T0001
//@ feature: COMPILER-TI-UNIFICATION, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
fn main() -> i32 {
    let x: i32 = 1;
    let y: bool = true;
    x + y //~ ERROR mismatched types
}
