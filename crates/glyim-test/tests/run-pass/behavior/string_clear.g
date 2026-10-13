// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut s = String::new();
    s.push_str("abc");
    s.clear();
    s.len() as i32
}
