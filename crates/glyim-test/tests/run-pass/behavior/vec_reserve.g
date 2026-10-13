// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    v.reserve(100);
    if v.capacity() >= v.len() { 1 } else { 0 }
}
