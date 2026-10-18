// FnDef -> FnPtr coercion as a call argument (value-position constant
// flowing into a `fn(i32)->i32` parameter, then called through the
// FnPtr path inside the callee).
// test-mode: run-pass
// exit-code: 53
// compile-flags: --with-stdlib
fn inc(x: i32) -> i32 { x + 1 }
fn apply(f: fn(i32) -> i32, v: i32) -> i32 { f(v) }
fn main() -> i32 {
    apply(inc, 52)
}
