// A `static` value read in the middle of an arithmetic expression, to
// catch any miscompile that folds the read to a placeholder.
// test-mode: run-pass
// exit-code: 59
// compile-flags: --with-stdlib
static BASE: i32 = 13;
fn main() -> i32 { BASE + BASE * 3 + 7 }
