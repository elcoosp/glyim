// `?` in helper, chained; Ok path.
// test-mode: run-pass
// exit-code: 11
// compile-flags: --with-stdlib
fn a() -> Result<i32, i32> { Result::Ok(1) }
fn b() -> Result<i32, i32> {
    let x = a()?;
    Result::Ok(x + 10)
}
fn main() -> i32 {
    let x = b()?;
    x
}
