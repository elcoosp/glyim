// test-mode: run-pass
// exit-code: 2
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1); v.push(2); v.push(3); v.push(4); v.push(5);
    v.truncate(2);
    v.len() as i32
}
