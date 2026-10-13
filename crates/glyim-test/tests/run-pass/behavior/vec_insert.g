// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    v.push(2);
    v.insert(1, 9);
    v.len() as i32
}
