//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-VEC, STD-ALLOC-VEC, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    if Vec::new().is_empty() { 1 } else { 0 }
}
