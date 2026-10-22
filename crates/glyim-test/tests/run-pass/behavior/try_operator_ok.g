// `?` on Result::Ok extracts the payload.
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x: Result<i32, i32> = Result::Ok(5);
    let y = x?;
    y
}
