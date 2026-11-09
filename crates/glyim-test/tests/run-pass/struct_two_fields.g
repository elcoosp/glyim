//@ mode: run-pass
//@ expect-exit: 30
//@ feature: LANG-SYN-STRUCT, LANG-SYN-FIELD-ACCESS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

struct Pair { a: i32, b: i32 }

fn main() -> i32 {
    let p = Pair { a: 10, b: 20 };
    p.a + p.b
}
