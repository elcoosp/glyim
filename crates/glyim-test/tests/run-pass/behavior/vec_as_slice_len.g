//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-SLICE, STD-CORE-SLICE, LANG-BUILTIN-VEC, STD-ALLOC-VEC, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    v.push(2);
    v.push(3);
    let s = v.as_slice();
    s.len() as i32
}
