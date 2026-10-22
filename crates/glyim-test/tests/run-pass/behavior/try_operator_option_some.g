// `?` on Option::Some extracts the payload.
// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x: Option<i32> = Option::Some(7);
    let y = x?;
    y
}
