// Closure combinator on Result (2-param generic).
// test-mode: run-pass
// exit-code: 8
// compile-flags: --with-stdlib
fn main() -> i32 {
    let r: Result<i32, i32> = Result::Ok(4);
    r.map(|x| x * 2).unwrap()
}
