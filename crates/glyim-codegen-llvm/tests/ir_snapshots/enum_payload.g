enum Shape { Circle(f64), Rect { w: i64, h: i64 } }
main = () => {
    let c = Shape::Circle(3.14);
    let r = Shape::Rect { w: 10, h: 20 };
    match c {
        Shape::Circle(rad) => 1,
        Shape::Rect { w, h } => w + h,
    }
}
