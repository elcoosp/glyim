// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1); v.push(2); v.push(3);
    unsafe { v.set_len(1); }
    v.len() as i32
}
