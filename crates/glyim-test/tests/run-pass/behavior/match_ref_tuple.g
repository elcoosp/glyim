// Regression: `ref` bindings in a tuple pattern.
// `(ref a, ref b) => *a + *b` reports "aggregate constant insert failed:
// ExtractOutOfRange". Pre-existing; the struct/tuple sub-pattern path
// doesn't support ref bindings yet.
// test-mode: run-pass
// exit-code: 30
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let pair = (10, 20);
    match pair {
        (ref a, ref b) => *a + *b,
    }
}
