//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-STRING, STD-ALLOC-STRING, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let s = String::from("abc");
    s.len() as i32
}
