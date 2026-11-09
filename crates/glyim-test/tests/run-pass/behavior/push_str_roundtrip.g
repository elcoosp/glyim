//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: STD-ALLOC-STRING, LANG-TY-STR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut s = String::new(); s.push_str("xy"); s.push_str("z"); s.to_string().len() as i32
}
