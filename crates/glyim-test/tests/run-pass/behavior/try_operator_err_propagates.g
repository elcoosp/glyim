// `?` on Result::Err propagates the error to the caller.
//@ mode: run-pass
//@ expect-exit: 42
//@ compile-flags: --with-stdlib
//@ feature: STD-CORE-RESULT, LANG-ERR-TRY-RESULT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn get() -> Result<i32, i32> { Result::Err(42) }
fn inner() -> Result<i32, i32> {
    let v = get()?;
    Result::Ok(v + 1)
}
fn main() -> i32 {
    match inner() {
        Result::Ok(v) => v,
        Result::Err(e) => e,
    }
}
