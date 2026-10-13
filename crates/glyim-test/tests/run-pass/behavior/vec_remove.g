// test-mode: run-pass
// exit-code: 2
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(10);
    v.push(20);
    v.push(30);
    v.remove(1);
    v.len() as i32
}
