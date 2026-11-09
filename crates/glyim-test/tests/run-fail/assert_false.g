//@ mode: run-fail
//@ expect-exit: 101
//@ feature: STD-CORE-PANIC, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
fn main() {
    assert!(false);
}
