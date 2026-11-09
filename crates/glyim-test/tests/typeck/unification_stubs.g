//@ mode: compile-pass
//@ feature: TOOL-TEST-COMPILE-PASS
//@ backends: gam

struct Point { x: i32, y: i32 }

fn main() -> i32 {
    let p = Point { x: 10, y: 20 };
    let a = p.x;        // field access
    a                   // returns 10 (but we only need it to compile)
}
