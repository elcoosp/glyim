// FnDef -> FnPtr coercion in value position: `let f: fn(..) = add`.
// test-mode: run-pass
// exit-code: 37
// compile-flags: --with-stdlib
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() -> i32 {
    let f: fn(i32, i32) -> i32 = add;
    f(11, 26)
}
