// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut s = String::new(); s.push_str("abc"); s.len() as i32
}
