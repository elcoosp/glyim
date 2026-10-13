// test-mode: compile-pass
struct Point { x: i32, y: i32 }
impl Point {
    fn sum(&self) -> i32 { self.x + self.y }
}
fn main() -> i32 {
    let p = Point { x: 1, y: 2 };
    p.sum()
}
