// §3.5.36-adjacent: bare `Some(x)` / `None` must resolve at the crate
// root without `Option::` qualification. The prelude's `pub use option::Option;`
// re-exports only the *type*; session 29 added variant-level re-exports
// (`pub use option::Option::Some;` etc.) so bare variants land in the root
// value namespace.
// test-mode: run-pass
// compile-flags: --with-stdlib
// exit-code: 42
fn main() -> i32 {
    let o: Option<i32> = Some(42);
    if let Some(x) = o { x } else { 0 }
}
