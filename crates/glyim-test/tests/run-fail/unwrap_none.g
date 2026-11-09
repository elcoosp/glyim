//@ mode: run-fail
//@ expect-exit: 101
//@ feature: STD-CORE-OPTION, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
fn main() -> i32 {
    let o = if 1 > 2 { Option::Some(1) } else { Option::None };
    o.unwrap()
}
