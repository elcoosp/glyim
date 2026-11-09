//@ mode: run-pass
//@ expect-exit: 2
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-SLICE, STD-CORE-SLICE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    v.push(2);
    v.push(3);
    v.push(4);
    let s = v.as_slice();
    let (l, _r) = s.split_at(2);
    l.len() as i32
}
