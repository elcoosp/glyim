// Regression: a ctor result stored in a local and then passed to a
// function. Guards the `variant_expr` sig input/output var-sharing fix
// (the shape that made `Option::or` ICE).
//@ mode: run-pass
//@ expect-exit: 8
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-TUPLE-STRUCT, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn consume(o: Option<i32>) -> i32 {
    o.unwrap_or(0)
}
fn main() -> i32 {
    let b = Option::Some(8);
    consume(b)
}
