// Regression: `ref mut` patterns (read path). The parser emits
// `ref`/`mut` as sibling tokens before `PatIdent`; detection must skip
// the `mut` to find the `ref`.
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut o = Option::Some(5);
    match o {
        Option::Some(ref mut v) => *v,
        Option::None => 0,
    }
}
