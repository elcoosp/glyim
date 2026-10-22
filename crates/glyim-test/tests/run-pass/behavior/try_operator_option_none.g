// `?` on Option::None propagates `None` to the caller.
// test-mode: run-pass
// exit-code: 99
// compile-flags: --with-stdlib
fn get() -> Option<i32> { Option::None }
fn inner() -> Option<i32> {
    let v = get()?;
    Option::Some(v + 1)
}
fn main() -> i32 {
    match inner() {
        Option::Some(v) => v,
        Option::None => 99,
    }
}
