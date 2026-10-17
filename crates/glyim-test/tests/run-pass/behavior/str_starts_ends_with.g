// ANTI-REGRESSION: `str::starts_with` / `str::ends_with` must exist and
// return the correct boolean.
//
// Both were missing from the stdlib surface (only `len`, `is_empty`,
// `contains`, `chars`, `lines`, `trim`, `parse` were defined). Added
// naive byte-scan implementations that match Rust's byte-substring
// semantics.
// test-mode: run-pass
// exit-code: 19
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a: i32 = if "hello".starts_with("he") { 1 } else { 0 };
    let b: i32 = if "hello".ends_with("lo") { 2 } else { 0 };
    let c: i32 = if "hello".starts_with("xx") { 4 } else { 0 };
    let d: i32 = if "hello".ends_with("xy") { 8 } else { 0 };
    let e: i32 = if "hello".starts_with("") { 16 } else { 0 };
    // 1 + 2 + 0 + 0 + 16 = 19
    a + b + c + d + e
}
