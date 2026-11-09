//@ mode: run-pass
//@ expect-exit: 10
//@ feature: LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn double(x: i32) -> i32 { x + x }
fn add(a: i32, b: i32) -> i32 { a + b }

fn main() -> i32 {
    add(double(3), 4)
}
