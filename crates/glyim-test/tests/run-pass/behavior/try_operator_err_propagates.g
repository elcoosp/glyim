// `?` on Result::Err propagates the error to the caller.
// test-mode: run-pass
// exit-code: 42
// compile-flags: --with-stdlib
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
