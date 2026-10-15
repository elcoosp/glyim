// Chained combinators: each map is a fresh resolve_method_call.
// test-mode: run-pass
// exit-code: 12
// compile-flags: --with-stdlib
fn main() -> i32 {
    Option::Some(1).map(|x| x + 1).map(|y| y * 6).unwrap()
}
