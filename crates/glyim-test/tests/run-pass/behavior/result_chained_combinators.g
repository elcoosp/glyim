// Regression: chained combinators on `Result` (2-param generic). Guards
// the method-generics + closure-seeding fix on a non-Option ADT.
// test-mode: run-pass
// exit-code: 8
// compile-flags: --with-stdlib
fn main() -> i32 {
    let r: Result<i32, i32> = Result::Ok(3);
    r.map(|x| x + 1).map(|y| y * 2).unwrap()
}
