// KNOWN FAILING — nested `Result<Result<i32, i32>, i32>`. Same
// `unexpected call return kind` ICE as nested_option.g.
//@ mode: run-pass
//@ ignore
//@ expect-exit: 4
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-RESULT, STD-CORE-RESULT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let r: Result<Result<i32, i32>, i32> = Result::Ok(Result::Ok(4));
    r.unwrap().unwrap()
}
