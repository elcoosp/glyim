// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut a = String::new(); a.push_str("aaaa"); let mut b = String::new(); b.push_str("bb"); (a.len() + b.len()) as i32
}
