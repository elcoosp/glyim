// Same impl, different method: exercises the candidate loop's non-matching
// iterations. `unwrap_or_else` on `None` invokes the closure → 11.
//@ mode: run-pass
//@ expect-exit: 11
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let o: Option<i32> = Option::None;
    o.unwrap_or_else(|| 11)
}
