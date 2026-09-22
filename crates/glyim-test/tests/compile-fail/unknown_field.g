// test-mode: compile-fail
struct Point { x: i32, y: i32 }
fn main() {
    let p = Point { x: 1, y: 2, z: 3 }; //~ ERROR no field `z`
}
