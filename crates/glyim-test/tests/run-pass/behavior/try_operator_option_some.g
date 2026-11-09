// `?` on Option::Some extracts the payload.
//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, LANG-ERR-TRY-RESULT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let x: Option<i32> = Option::Some(7);
    let y = x?;
    y
}
