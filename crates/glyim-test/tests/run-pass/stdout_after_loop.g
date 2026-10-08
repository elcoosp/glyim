// test-mode: run-pass
// compile-flags: --with-stdlib
// check-stdout: done
fn main() {
    let mut i = 0;
    while i < 3 {
        i = i + 1;
    }
    if i == 3 { println("done"); }
}
