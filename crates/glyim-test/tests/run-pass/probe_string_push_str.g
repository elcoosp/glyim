// test-mode: run-pass
// compile-flags: --with-stdlib
// check-stdout: abc
fn main() {
    let mut s = String::new();
    s.push_str("abc");
    println!("{}", s.as_str());
}
