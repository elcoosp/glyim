// Regression: a ctor result stored in a local and then passed to a
// function. Guards the `variant_expr` sig input/output var-sharing fix
// (the shape that made `Option::or` ICE).
// test-mode: run-pass
// exit-code: 8
// compile-flags: --with-stdlib
fn consume(o: Option<i32>) -> i32 {
    o.unwrap_or(0)
}
fn main() -> i32 {
    let b = Option::Some(8);
    consume(b)
}
