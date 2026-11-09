// Regression: `Option::map` with a closure. Two compounding causes:
// callee FnDef substs missing method generics; closure arg unseeded.
//@ mode: run-pass
//@ expect-exit: 6
//@ compile-flags: --with-stdlib
//@ feature: LANG-CLOSURE-EXPR, LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let o = Option::Some(5);
    let r = o.map(|x| x + 1);
    r.unwrap()
}
