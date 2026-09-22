// Enum payload extracted, then fed into a chain of arithmetic.
// Combines the enum-aggregate-fold fix, the Field-offset fix, and the
// binary-expr checkpoint fix.
// test-mode: run-pass
// exit-code: 20

enum Wrapper { Value(i32), Empty }

fn main() -> i32 {
    let w = Wrapper::Value(10);
    let v = match w {
        Wrapper::Value(x) => x,
        Wrapper::Empty => 0,
    };
    v + v
}
