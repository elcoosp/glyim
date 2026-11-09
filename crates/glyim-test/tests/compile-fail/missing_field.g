//@ mode: compile-fail
//@ feature: LANG-SYN-FIELD-ACCESS, COMPILER-FRONT-DEFTABLE, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
struct Point { x: i32, y: i32 }
fn main() {
    let p = Point { x: 1 }; //~ ERROR missing field
}
