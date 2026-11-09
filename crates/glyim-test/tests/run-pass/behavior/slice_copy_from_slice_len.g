//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-SLICE, STD-CORE-SLICE, STD-CORE-MARKER, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut a = Vec::new();
    a.push(1);
    a.push(2);
    a.push(3);
    let mut b = Vec::new();
    b.push(0);
    b.push(0);
    b.push(0);
    let src = a.as_slice();
    let dst = b.as_slice();
    dst.copy_from_slice(src);
    b.len() as i32
}
