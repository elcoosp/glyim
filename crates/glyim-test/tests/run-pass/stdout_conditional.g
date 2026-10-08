// test-mode: run-pass
// compile-flags: --with-stdlib
// check-stdout: positive
fn main() {
    let n = 5;
    if n > 0 {
        println("positive");
    } else {
        println("non-positive");
    }
}
