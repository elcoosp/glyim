// `?` on Result::Ok extracts the payload.
//@ mode: run-pass
//@ expect-exit: 5
//@ compile-flags: --with-stdlib
//@ feature: STD-CORE-RESULT, LANG-ERR-TRY-RESULT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let x: Result<i32, i32> = Result::Ok(5);
    let y = x?;
    y
}
