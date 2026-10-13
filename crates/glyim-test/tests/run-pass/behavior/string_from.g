// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    let s = String::from("abc");
    s.len() as i32
}
