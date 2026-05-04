enum Shape { Circle(f64), Square(i64) }
main = () => {
    let c = Shape::Circle(3.14);
    match c {
        Shape::Circle(_) => 1,
        Shape::Square(_) => 2
    }
}
