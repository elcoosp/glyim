//@ mode: run-pass
//@ expect-exit: 20
//@ feature: LANG-SYN-STRUCT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct P { x: i32, y: i32 }
fn main() -> i32 {
    let p = P { x: 10, y: 20 };
    p.y
}
