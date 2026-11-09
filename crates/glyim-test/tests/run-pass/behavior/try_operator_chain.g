// `?` in helper, chained; Ok path.
//@ mode: run-pass
//@ expect-exit: 11
//@ compile-flags: --with-stdlib
//@ feature: LANG-ERR-TRY-RESULT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn a() -> Result<i32, i32> { Result::Ok(1) }
fn b() -> Result<i32, i32> {
    let x = a()?;
    Result::Ok(x + 10)
}
fn main() -> i32 {
    let x = b()?;
    x
}
