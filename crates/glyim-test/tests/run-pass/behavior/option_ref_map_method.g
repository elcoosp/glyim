// Closure param must be seeded with the *resolved* formal (concrete
// `&str`), not a raw Infer var — else `p.len()` reports no-method.
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 {
    let s = "hello";
    let o: Option<&str> = Option::Some(s);
    o.map(|p| p.len() as i32).unwrap()
}
