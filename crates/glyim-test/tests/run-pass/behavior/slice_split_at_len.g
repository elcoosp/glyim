// test-mode: run-pass
// exit-code: 2
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    v.push(2);
    v.push(3);
    v.push(4);
    let s = v.as_slice();
    let (l, _r) = s.split_at(2);
    l.len() as i32
}
