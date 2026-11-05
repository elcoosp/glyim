// test-mode: run-pass
// compile-flags: --with-stdlib
// exit-code: 10
fn main() -> i32 {
    let s: &[i32] = &[10, 20, 30];
    let mut it = s.iter();
    match it.next() { Some(x) => *x, None => 0 }
}
