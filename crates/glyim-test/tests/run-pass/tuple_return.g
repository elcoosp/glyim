//@ mode: run-pass
//@ expect-exit: 22
//@ feature: LANG-SYN-RETURN, LANG-TY-TUPLE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn pair() -> (i32, i32) { (11, 22) }

fn main() -> i32 {
    let (a, b) = pair();
    b
}
