// test-mode: run-pass
// check-stdout: abc
fn main() {
    let mut s = String::new();
    s.push_str("abc");
    println!("{}", s.as_str());
}
