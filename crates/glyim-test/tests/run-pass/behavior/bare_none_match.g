// §3.5.36-adjacent: bare `None` in a match arm.
// test-mode: run-pass
// compile-flags: --with-stdlib
// exit-code: 0
fn main() -> i32 {
    let o: Option<i32> = None;
    match o { Some(x) => x, None => 0 }
}
