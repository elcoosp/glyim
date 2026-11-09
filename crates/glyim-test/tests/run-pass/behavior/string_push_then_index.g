//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: STD-ALLOC-STRING, LANG-BUILTIN-STRING, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut s = String::new(); s.push_str("abc"); s.as_str().len() as i32
}
