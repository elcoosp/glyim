// unreachable!() message. §3.5.28 acceptance.
// test-mode: run-fail
// exit-code: 101
// check-stderr: thread 'main' panicked: internal error: entered unreachable code
// compile-flags: --with-stdlib
fn main() -> i32 {
    unreachable!()
}
