//@ mode: run-fail
//@ expect-exit: 101
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
fn main() -> i32 {
    let d = 0;
    1 / d
}
