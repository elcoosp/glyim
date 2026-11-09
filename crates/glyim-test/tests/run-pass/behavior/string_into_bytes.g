//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-STRING, STD-ALLOC-STRING, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut s = String::new();
    s.push_str("abc");
    let b = s.into_bytes();
    b.len() as i32
}
