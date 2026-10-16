// KNOWN GAP (NOT a fixed bug): using the parsed value in a `match` instead
// of discarding it currently hits `internal compiler error:
// TyKind::Projection reached LLVM codegen – type normalization incomplete`
// because `<T as FromStr>::Err` is not normalized before codegen when the
// `Err` payload is pattern-matched.
//
// This fixture is a placeholder that documents the gap. When the codegen
// projection-normalization pass lands, remove the `// ignore` line so the
// test starts exercising the fixed path.
// test-mode: ui
// ignore
// compile-flags: --with-stdlib
fn main() -> i32 {
    let r = "42".parse::<i32>();
    match r {
        Result::Ok(v) => v,
        Result::Err(_) => -1,
    }
}
