//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SYN-STRUCT, LANG-SYN-FIELD-ACCESS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

struct Inner { value: i32 }
struct Outer { inner: Inner }

fn main() -> i32 {
    let o = Outer { inner: Inner { value: 42 } };
    o.inner.value
}
