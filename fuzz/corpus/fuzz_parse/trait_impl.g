// test-mode: compile-pass
trait Shape {
    fn area(&self) -> i32;
}
struct Square { side: i32 }
impl Shape for Square {
    fn area(&self) -> i32 { self.side * self.side }
}
fn main() -> i32 {
    let s = Square { side: 3 };
    s.area()
}
