// Chained combinators: each map is a fresh resolve_method_call.
//@ mode: run-pass
//@ expect-exit: 12
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    Option::Some(1).map(|x| x + 1).map(|y| y * 6).unwrap()
}
