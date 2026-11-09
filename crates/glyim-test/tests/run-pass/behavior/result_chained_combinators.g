// Regression: chained combinators on `Result` (2-param generic). Guards
// the method-generics + closure-seeding fix on a non-Option ADT.
//@ mode: run-pass
//@ expect-exit: 8
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-OPERATORS, LANG-BUILTIN-RESULT, STD-CORE-RESULT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let r: Result<i32, i32> = Result::Ok(3);
    r.map(|x| x + 1).map(|y| y * 2).unwrap()
}
