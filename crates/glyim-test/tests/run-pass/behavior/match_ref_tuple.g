// Regression: `ref` bindings in a tuple pattern.
// `(ref a, ref b) => *a + *b` reports "aggregate constant insert failed:
// ExtractOutOfRange". Pre-existing; the struct/tuple sub-pattern path
// doesn't support ref bindings yet.
//@ mode: run-pass
//@ expect-exit: 30
//@ compile-flags: --with-stdlib
//@ needs-llvm
//@ feature: LANG-MATCH-GUARDS, LANG-TY-TUPLE, LANG-TY-REF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let pair = (10, 20);
    match pair {
        (ref a, ref b) => *a + *b,
    }
}
