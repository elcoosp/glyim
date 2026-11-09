// Fn-pointer-valued static: same as fn_const_read but through the
// statics feature. `scan_const` recurses into the StaticRef's `init`
// so `add`'s body is enqueued and the emitted global's initializer
// resolves at link.
//@ mode: run-pass
//@ expect-exit: 53
//@ compile-flags: --with-stdlib
//@ feature: LANG-SCOPE-STATIC-ITEM, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn add3(a: i32, b: i32, c: i32) -> i32 { a + b + c }
static F: fn(i32, i32, i32) -> i32 = add3;
fn main() -> i32 { F(17, 19, 17) }
