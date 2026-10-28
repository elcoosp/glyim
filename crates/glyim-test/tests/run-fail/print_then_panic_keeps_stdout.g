// TEST-7 regression: a program that prints *before* panicking must keep
// its output in the interpreter path. Under the old shape the `Err` arm of
// `interpret_bodies` returned `stdout: []`, so `before` was silently
// dropped. The fix hoists the buffer reads above the match, so both arms
// see whatever the program wrote.
// test-mode: run-fail
// exit-code: 101
// check-stdout: before
// compile-flags: --with-stdlib
fn main() -> i32 {
    println("before");
    assert!(false, "boom");
    0
}
