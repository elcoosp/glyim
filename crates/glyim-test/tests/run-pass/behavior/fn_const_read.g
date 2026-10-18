// Fn-pointer-valued constant: `const F: fn(..) = add;` folds the
// path-to-fn initializer to a `MirConstKind::Fn` via the new
// `ConstValue::Fn` const-evaluator variant, then calls through it.
// test-mode: run-pass
// exit-code: 30
// compile-flags: --with-stdlib
fn add(a: i32, b: i32) -> i32 { a + b }
const F: fn(i32, i32) -> i32 = add;
fn main() -> i32 { F(10, 20) }
