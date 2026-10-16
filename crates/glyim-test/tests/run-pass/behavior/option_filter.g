// Regression: `Option::filter` (added to option.g). Needs
// `fn(&T) -> bool` closure params + a `ref` pattern in the body.
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let o = Option::Some(5);
    o.filter(|x| *x > 0).unwrap()
}
