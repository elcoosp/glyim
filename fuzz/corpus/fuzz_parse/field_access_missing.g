// test-mode: compile-fail
struct Point { x: i32, y: i32 }
fn main() -> i32 {
    let p = Point { x: 10, y: 20 };
    p.z //~ ERROR no field
}
