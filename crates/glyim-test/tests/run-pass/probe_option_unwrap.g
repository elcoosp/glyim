//@ mode: run-pass
//@ expect-exit: 5
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let o = Option::Some(5);
    o.unwrap()
}
