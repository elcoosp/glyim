// §3.5.33: match expression whose every arm body diverges (`!`) types as
// `!`, not `()`. Before the fix, this gave "mismatched types: () vs i32"
// against the enclosing fn's return type.
//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: LANG-MATCH-GUARDS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    match Option::Some(5) {
        Option::Some(_) => return 0,
        Option::None => return 1,
    }
}
