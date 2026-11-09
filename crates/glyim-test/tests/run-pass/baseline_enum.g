//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SYN-ENUM, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
enum E { A(i32), B }
fn main() -> i32 {
    let e = E::A(42);
    match e { E::A(v) => v, E::B => 0 }
}
