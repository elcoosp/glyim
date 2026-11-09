//@ mode: run-pass
//@ expect-exit: 3
//@ feature: LANG-SYN-IF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 {
    let x = 5;
    if x > 3 { 3 } else { 0 }
}
