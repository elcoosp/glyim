// KNOWN FAILING — nested `Option<Option<i32>>`.
// `Option::Some(Option::Some(6)).unwrap().unwrap()` ICEs with
// `[X0000] unexpected call return kind`. The inner `unwrap` returns
// `Option<i32>`; codegen's call-return-kind switch doesn't handle the
// aggregate-returning call shape at this nesting depth.
//@ mode: run-pass
//@ ignore
//@ expect-exit: 6
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    Option::Some(Option::Some(6)).unwrap().unwrap()
}
