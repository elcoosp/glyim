// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
fn main() -> i32 {
    let f = || 7;
    f()
}
