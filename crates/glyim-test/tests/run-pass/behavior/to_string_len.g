// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
fn main() -> i32 {
    ("abc").to_string().len() as i32
}
