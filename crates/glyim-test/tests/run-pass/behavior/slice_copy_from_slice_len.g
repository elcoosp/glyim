// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut a = Vec::new();
    a.push(1);
    a.push(2);
    a.push(3);
    let mut b = Vec::new();
    b.push(0);
    b.push(0);
    b.push(0);
    let src = a.as_slice();
    let dst = b.as_slice();
    dst.copy_from_slice(src);
    b.len() as i32
}
