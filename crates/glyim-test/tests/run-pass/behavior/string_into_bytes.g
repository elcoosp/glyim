// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut s = String::new();
    s.push_str("abc");
    let b = s.into_bytes();
    b.len() as i32
}
