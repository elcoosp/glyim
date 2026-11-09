// Regression: a closure whose annotated `&str` param has a method called
// on it. Guards both the annotation fix and the resolved-formal seeding.
//@ mode: run-pass
//@ expect-exit: 5
//@ compile-flags: --with-stdlib
//@ feature: LANG-CLOSURE-EXPR, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let f = |s: &str| s.len() as i32;
    f("hello")
}
