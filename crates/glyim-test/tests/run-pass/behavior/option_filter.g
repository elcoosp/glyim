// Regression: `Option::filter` (added to option.g). Needs
// `fn(&T) -> bool` closure params + a `ref` pattern in the body.
//@ mode: run-pass
//@ expect-exit: 5
//@ compile-flags: --with-stdlib
//@ needs-llvm
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let o = Option::Some(5);
    o.filter(|x| *x > 0).unwrap()
}
