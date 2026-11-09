// `?` on Option::None propagates `None` to the caller.
//@ mode: run-pass
//@ expect-exit: 99
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, LANG-ERR-TRY-RESULT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn get() -> Option<i32> { Option::None }
fn inner() -> Option<i32> {
    let v = get()?;
    Option::Some(v + 1)
}
fn main() -> i32 {
    match inner() {
        Option::Some(v) => v,
        Option::None => 99,
    }
}
