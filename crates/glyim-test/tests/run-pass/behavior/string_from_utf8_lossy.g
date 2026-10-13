// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(97u8);
    v.push(98u8);
    v.push(99u8);
    let s = v.as_slice();
    let st = String::from_utf8_lossy(s);
    st.len() as i32
}
