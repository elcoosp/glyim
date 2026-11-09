// Closure combinator on Result (2-param generic).
//@ mode: run-pass
//@ expect-exit: 8
//@ compile-flags: --with-stdlib
//@ feature: LANG-CLOSURE-EXPR, LANG-BUILTIN-RESULT, STD-CORE-RESULT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let r: Result<i32, i32> = Result::Ok(4);
    r.map(|x| x * 2).unwrap()
}
