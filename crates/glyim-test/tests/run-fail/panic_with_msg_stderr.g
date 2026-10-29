// Direct panic! with a literal message. §3.5.28 acceptance.
// test-mode: run-fail
// exit-code: 101
// check-stderr: thread 'main' panicked: boom
// compile-flags: --with-stdlib
fn main() -> i32 {
    panic!("boom")
}
