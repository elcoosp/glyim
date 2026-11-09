//@ mode: compile-pass
//@ compile-flags: --backend=llvm
//@ expect-stdout: 2
//@ feature: TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() -> i32 {
    let arr = [1, 2, 3];
    arr[1]  // should be 2
}
