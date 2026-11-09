//@ mode: run-pass
//@ expect-exit: 7
//@ feature: LANG-SYN-RETURN, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn classify(n: i32) -> i32 {
    if n < 0 { return 0; }
    if n > 5 { return 7; }
    n
}

fn main() -> i32 { classify(10) }
