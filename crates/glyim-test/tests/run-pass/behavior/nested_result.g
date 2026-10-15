// KNOWN FAILING — nested `Result<Result<i32, i32>, i32>`. Same
// `unexpected call return kind` ICE as nested_option.g.
// test-mode: run-pass
// ignore
// exit-code: 4
// compile-flags: --with-stdlib
fn main() -> i32 {
    let r: Result<Result<i32, i32>, i32> = Result::Ok(Result::Ok(4));
    r.unwrap().unwrap()
}
