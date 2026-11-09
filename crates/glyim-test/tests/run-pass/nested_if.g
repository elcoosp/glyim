//@ mode: run-pass
//@ expect-exit: 100
//@ feature: LANG-SYN-IF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 {
    let a = 5;
    let b = 10;
    if a > 3 {
        if b > 5 {
            100
        } else {
            50
        }
    } else {
        0
    }
}
