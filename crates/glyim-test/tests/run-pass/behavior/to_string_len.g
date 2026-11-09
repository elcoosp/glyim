//@ mode: run-pass
//@ expect-exit: 3
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-STRING, STD-ALLOC-STRING, STD-STD-INT-STR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    ("abc").to_string().len() as i32
}
