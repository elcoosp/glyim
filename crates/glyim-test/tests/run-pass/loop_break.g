//@ mode: run-pass
//@ expect-exit: 3
//@ feature: LANG-SYN-LOOP-BREAK, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 {
    let mut i = 0;
    loop {
        if i == 3 { break; }
        i = i + 1;
    }
    i
}
