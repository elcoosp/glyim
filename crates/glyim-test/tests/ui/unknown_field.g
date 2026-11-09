//@ mode: ui
//@ feature: LANG-SYN-FIELD-ACCESS, COMPILER-FRONT-DEFTABLE, TOOL-TEST-UI
//@ backends: gam
struct Point { x: i32, y: i32 }
fn main() {
    let p = Point { x: 1, y: 2, z: 3 };
}
