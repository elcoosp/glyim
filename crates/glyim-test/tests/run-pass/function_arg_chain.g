//@ mode: run-pass
//@ expect-exit: 6
//@ feature: LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn step1(x: i32) -> i32 { x + 1 }
fn step2(x: i32) -> i32 { step1(x) * 2 }
fn step3(x: i32) -> i32 { step2(x) + step1(x) }

fn main() -> i32 {
    step3(1)
}
