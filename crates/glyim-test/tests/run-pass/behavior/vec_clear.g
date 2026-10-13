// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    v.push(2);
    v.clear();
    v.len() as i32
}
