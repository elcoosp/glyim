//@ mode: run-pass
//@ expect-exit: 5
//@ feature: LANG-MATCH-GUARDS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 {
    let x = 5;
    match x {
        n if n > 10 => 100,
        n if n > 3 => 5,
        _ => 0,
    }
}
