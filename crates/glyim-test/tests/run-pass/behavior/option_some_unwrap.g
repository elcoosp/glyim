// Regression: `Option::Some(5).unwrap()` used to ICE with
// `[X0000] layout error building aggregate: UnknownType(Ty(0))`.
// Root cause: resolve_method_call's candidate loop committed receiver
// unify bindings from impls that had no matching method.
//@ mode: run-pass
//@ expect-exit: 5
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let o = Option::Some(5);
    o.unwrap()
}
