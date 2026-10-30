// assert!(false) single-arg (stringify! path) — companion fixture.
// test-mode: run-fail
// exit-code: 101
// check-stderr: thread 'main' panicked: assertion failed: false
// compile-flags: --with-stdlib
fn main() -> i32 {
    assert!(false);
    0
}
