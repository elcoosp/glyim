// §3.5.33: match expression whose every arm body diverges (`!`) types as
// `!`, not `()`. Before the fix, this gave "mismatched types: () vs i32"
// against the enclosing fn's return type.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    match Option::Some(5) {
        Option::Some(_) => return 0,
        Option::None => return 1,
    }
}
