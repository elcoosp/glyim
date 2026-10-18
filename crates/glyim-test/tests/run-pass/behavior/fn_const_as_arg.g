// Fn-pointer-valued constant passed through to a `fn(..)` parameter,
// then called through the FnPtr path inside the callee.
// test-mode: run-pass
// exit-code: 42
// compile-flags: --with-stdlib
fn double(x: i32) -> i32 { x * 2 }
const D: fn(i32) -> i32 = double;
fn apply(f: fn(i32) -> i32, v: i32) -> i32 { f(v) }
fn main() -> i32 { apply(D, 21) }
