//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-VEC, STD-ALLOC-VEC, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    v.reserve(100);
    if v.capacity() >= v.len() { 1 } else { 0 }
}
