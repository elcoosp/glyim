// Option::None.unwrap() message. §3.5.28 acceptance.
// test-mode: run-fail
// exit-code: 101
// check-stderr: thread 'main' panicked: called `Option::unwrap()` on a `None` value
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x: Option<i32> = Option::None;
    x.unwrap()
}
