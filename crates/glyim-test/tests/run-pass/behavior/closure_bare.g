// Closures now lower inline: the closure body is emitted as
// `__glyim_fn_{closure_id}` with the parameter locals shared between the
// prologue and the body, and the call site passes the incoming argument.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let f = |x: i32| -> i32 { x + 1 };
    f(5)
}
