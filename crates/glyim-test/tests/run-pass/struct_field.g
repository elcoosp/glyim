//@ mode: run-pass
//@ expect-exit: 20
//@ feature: LANG-SYN-STRUCT, LANG-SYN-FIELD-ACCESS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

struct Point { x: i32, y: i32 }

fn main() -> i32 {
    let p = Point { x: 10, y: 20 };
    p.y
}
