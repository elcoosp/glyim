//@ mode: run-pass
//@ expect-exit: 1127
//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN
//@ backends: gam, llvm
fn main() -> i32 {
    let a0 = 20;
    let a1 = a0 + 9;
    let a2 = a1 + 20;
    let a3 = a2 + 60;
    let a4 = a3 + 65;
    let a5 = a4 + 21;
    let a6 = a5 + 53;
    let a7 = a6 + 13;
    let a8 = a7 + 64;
    let a9 = a8 + 34;
    let a10 = a9 + 14;
    let a11 = a10 + 32;
    let a12 = a11 + 67;
    let a13 = a12 + 20;
    let a14 = a13 + 38;
    let a15 = a14 + 18;
    let a16 = a15 + 51;
    let a17 = a16 + 36;
    let a18 = a17 + 12;
    let a19 = a18 + 45;
    let a20 = a19 + 37;
    let a21 = a20 + 7;
    let a22 = a21 + 33;
    let a23 = a22 + 47;
    let a24 = a23 + 8;
    let a25 = a24 + 31;
    let a26 = a25 + 26;
    let a27 = a26 + 61;
    let a28 = a27 + 54;
    let a29 = a28 + 64;
    let a30 = a29 + 25;
    let a31 = a30 + 42;
    a31
}
