// §3.5.36-adjacent: bare `Ok(x)` / `Err(x)` must resolve at the crate root.
// test-mode: run-pass
// compile-flags: --with-stdlib
// exit-code: 42
fn main() -> i32 {
    let r: Result<i32, i32> = Ok(42);
    match r { Ok(x) => x, Err(_) => 0 }
}
