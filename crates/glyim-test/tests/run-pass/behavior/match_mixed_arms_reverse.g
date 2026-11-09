// §3.5.33 companion: mixed divergent-SECOND arm. Scrutinee is Some(5),
// so the value arm (first) fires and exits 0. The divergent arm
// (None => return 99, second) is still type-checked, exercising the fix.
//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: LANG-MATCH-GUARDS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    match Option::Some(5) {
        Option::Some(_) => 0,
        Option::None => return 99,
    }
}
