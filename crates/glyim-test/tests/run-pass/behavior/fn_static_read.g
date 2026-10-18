// Fn-pointer-valued static: same as fn_const_read but through the
// statics feature. `scan_const` recurses into the StaticRef's `init`
// so `add`'s body is enqueued and the emitted global's initializer
// resolves at link.
// test-mode: run-pass
// exit-code: 53
// compile-flags: --with-stdlib
fn add3(a: i32, b: i32, c: i32) -> i32 { a + b + c }
static F: fn(i32, i32, i32) -> i32 = add3;
fn main() -> i32 { F(17, 19, 17) }
