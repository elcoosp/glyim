// `String::from_utf8(Vec<u8>) -> Result<String, Vec<u8>>`, valid-UTF-8 path:
// the result is `Ok(s)`, and `s` holds the same bytes.
// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(104u8);
    v.push(105u8);
    let s = String::from_utf8(v);
    if s.is_ok() { 1 } else { 0 }
}
