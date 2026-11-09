//@ mode: run-pass
//@ expect-exit: 4
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 {
    let a = 5;
    let b = 3;
    (a + b) - (a - b) - (a - b)
}
