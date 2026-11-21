//@ mode: run-pass
//@ expect-exit: 1127
//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN
//@ backends: gam, llvm
fn id<T>(x: T) -> T { x }

fn main() -> i32 {
    let v0 = id(20);
    let v1 = id(v0) + 9;
    let v2 = id(v1) + 20;
    let v3 = id(v2) + 60;
    let v4 = id(v3) + 65;
    let v5 = id(v4) + 21;
    let v6 = id(v5) + 53;
    let v7 = id(v6) + 13;
    let v8 = id(v7) + 64;
    let v9 = id(v8) + 34;
    let v10 = id(v9) + 14;
    let v11 = id(v10) + 32;
    let v12 = id(v11) + 67;
    let v13 = id(v12) + 20;
    let v14 = id(v13) + 38;
    let v15 = id(v14) + 18;
    let v16 = id(v15) + 51;
    let v17 = id(v16) + 36;
    let v18 = id(v17) + 12;
    let v19 = id(v18) + 45;
    let v20 = id(v19) + 37;
    let v21 = id(v20) + 7;
    let v22 = id(v21) + 33;
    let v23 = id(v22) + 47;
    let v24 = id(v23) + 8;
    let v25 = id(v24) + 31;
    let v26 = id(v25) + 26;
    let v27 = id(v26) + 61;
    let v28 = id(v27) + 54;
    let v29 = id(v28) + 64;
    let v30 = id(v29) + 25;
    let v31 = id(v30) + 42;
    v31
}
