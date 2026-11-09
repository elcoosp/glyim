//@ mode: run-pass
//@ expect-exit: 10
//@ feature: LANG-SYN-WHILE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 {
    let mut i = 0;
    let mut sum = 0;
    while i < 5 {
        sum = sum + i;
        i = i + 1;
    }
    sum
}
