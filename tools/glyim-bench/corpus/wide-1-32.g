//@ mode: run-pass
//@ expect-exit: 1271
//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN
//@ backends: gam, llvm
fn main() -> i32 {
    let a0 = 56;
    let a1 = a0 + 58;
    let a2 = a1 + 64;
    let a3 = a2 + 34;
    let a4 = a3 + 45;
    let a5 = a4 + 43;
    let a6 = a5 + 40;
    let a7 = a6 + 28;
    let a8 = a7 + 39;
    let a9 = a8 + 40;
    let a10 = a9 + 18;
    let a11 = a10 + 17;
    let a12 = a11 + 64;
    let a13 = a12 + 45;
    let a14 = a13 + 42;
    let a15 = a14 + 9;
    let a16 = a15 + 52;
    let a17 = a16 + 39;
    let a18 = a17 + 53;
    let a19 = a18 + 31;
    let a20 = a19 + 9;
    let a21 = a20 + 60;
    let a22 = a21 + 45;
    let a23 = a22 + 10;
    let a24 = a23 + 34;
    let a25 = a24 + 67;
    let a26 = a25 + 7;
    let a27 = a26 + 43;
    let a28 = a27 + 57;
    let a29 = a28 + 20;
    let a30 = a29 + 46;
    let a31 = a30 + 56;
    a31
}
