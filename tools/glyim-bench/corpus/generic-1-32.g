//@ mode: run-pass
//@ expect-exit: 1271
//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN
//@ backends: gam, llvm
fn id<T>(x: T) -> T { x }

fn main() -> i32 {
    let v0 = id(56);
    let v1 = id(v0) + 58;
    let v2 = id(v1) + 64;
    let v3 = id(v2) + 34;
    let v4 = id(v3) + 45;
    let v5 = id(v4) + 43;
    let v6 = id(v5) + 40;
    let v7 = id(v6) + 28;
    let v8 = id(v7) + 39;
    let v9 = id(v8) + 40;
    let v10 = id(v9) + 18;
    let v11 = id(v10) + 17;
    let v12 = id(v11) + 64;
    let v13 = id(v12) + 45;
    let v14 = id(v13) + 42;
    let v15 = id(v14) + 9;
    let v16 = id(v15) + 52;
    let v17 = id(v16) + 39;
    let v18 = id(v17) + 53;
    let v19 = id(v18) + 31;
    let v20 = id(v19) + 9;
    let v21 = id(v20) + 60;
    let v22 = id(v21) + 45;
    let v23 = id(v22) + 10;
    let v24 = id(v23) + 34;
    let v25 = id(v24) + 67;
    let v26 = id(v25) + 7;
    let v27 = id(v26) + 43;
    let v28 = id(v27) + 57;
    let v29 = id(v28) + 20;
    let v30 = id(v29) + 46;
    let v31 = id(v30) + 56;
    v31
}
