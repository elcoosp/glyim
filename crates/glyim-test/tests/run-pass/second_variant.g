// The other variant must be selected by the discriminant.
//@ mode: run-pass
//@ expect-exit: 7
//@ feature: TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

enum E { A(i32), B(i32) }

fn main() -> i32 {
    let e = E::B(7);
    match e {
        E::A(_) => 0,
        E::B(v) => v,
    }
}
