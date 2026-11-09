// Enum payload extracted, then fed into a chain of arithmetic.
// Combines the enum-aggregate-fold fix, the Field-offset fix, and the
// binary-expr checkpoint fix.
//@ mode: run-pass
//@ expect-exit: 20
//@ feature: LANG-SYN-ENUM, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

enum Wrapper { Value(i32), Empty }

fn main() -> i32 {
    let w = Wrapper::Value(10);
    let v = match w {
        Wrapper::Value(x) => x,
        Wrapper::Empty => 0,
    };
    v + v
}
