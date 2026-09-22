// test-mode: run-pass
// exit-code: 8

enum Shape { Circle(i32), Square(i32) }

fn area(s: Shape) -> i32 {
    match s {
        Shape::Circle(r) => r,
        Shape::Square(s) => s + s,
    }
}

fn main() -> i32 {
    area(Shape::Circle(8))
}
