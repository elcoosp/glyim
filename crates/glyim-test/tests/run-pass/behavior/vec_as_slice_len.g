// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    v.push(2);
    v.push(3);
    let s = v.as_slice();
    s.len() as i32
}
