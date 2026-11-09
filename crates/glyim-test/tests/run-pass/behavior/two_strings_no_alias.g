//@ mode: run-pass
//@ expect-exit: 6
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-STRING, STD-ALLOC-STRING, LANG-TY-REF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut a = String::new(); a.push_str("aaaa"); let mut b = String::new(); b.push_str("bb"); (a.len() + b.len()) as i32
}
