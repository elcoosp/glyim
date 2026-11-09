//@ mode: run-pass
//@ expect-exit: 8
//@ feature: LANG-MATCH-GUARDS, LANG-SYN-ENUM, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

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
