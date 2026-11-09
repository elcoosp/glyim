//@ mode: run-pass
//@ expect-exit: 2
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-VEC, STD-ALLOC-VEC, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(10);
    v.push(20);
    v.push(30);
    v.remove(1);
    v.len() as i32
}
