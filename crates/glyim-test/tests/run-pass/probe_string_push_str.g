//@ mode: run-pass
//@ compile-flags: --with-stdlib
//@ expect-stdout: abc
//
//@ feature: STD-ALLOC-STRING, LANG-BUILTIN-STRING, LANG-TY-STR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() {
    let mut s = String::new();
    s.push_str("abc");
    println!("{}", s.as_str());
}
