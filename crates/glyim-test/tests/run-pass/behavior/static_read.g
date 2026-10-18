// By-value read of a `static` item whose initializer const-evaluates.
// test-mode: run-pass
// exit-code: 71
// compile-flags: --with-stdlib
static VALUE: i32 = 71;
fn main() -> i32 { VALUE }
