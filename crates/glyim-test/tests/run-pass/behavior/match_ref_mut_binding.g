// Regression: `ref mut` patterns (read path). The parser emits
// `ref`/`mut` as sibling tokens before `PatIdent`; detection must skip
// the `mut` to find the `ref`.
//@ mode: run-pass
//@ expect-exit: 5
//@ compile-flags: --with-stdlib
//@ needs-llvm
//@ feature: LANG-MATCH-GUARDS, LANG-TY-REF-MUT, LANG-SYN-MUT, LANG-TY-REF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut o = Option::Some(5);
    match o {
        Option::Some(ref mut v) => *v,
        Option::None => 0,
    }
}
