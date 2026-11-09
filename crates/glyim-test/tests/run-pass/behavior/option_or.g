// Regression: `Option::or(self, optb: Option<T>) -> Option<T>`.
// `let b = Option::Some(8)` (no expected type) used to declare `b` as
// `Adt1010<<error>>` in MIR — variant_expr registered the ctor's sig
// with a rigid `Param(0)` input alongside an `Adt(id, [Infer(X)])`
// output, so X was never bound. Passing `b` to `.or()` then ICEd.
// Fixed by sharing the ctor vars between sig input and output.
//@ mode: run-pass
//@ expect-exit: 8
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let a: Option<i32> = Option::None;
    let b = Option::Some(8);
    a.or(b).unwrap()
}
