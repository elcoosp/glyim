// KNOWN FAILING — nested `Option<Option<i32>>`.
// `Option::Some(Option::Some(6)).unwrap().unwrap()` ICEs with
// `[X0000] unexpected call return kind`. The inner `unwrap` returns
// `Option<i32>`; codegen's call-return-kind switch doesn't handle the
// aggregate-returning call shape at this nesting depth.
// test-mode: run-pass
// ignore
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    Option::Some(Option::Some(6)).unwrap().unwrap()
}
