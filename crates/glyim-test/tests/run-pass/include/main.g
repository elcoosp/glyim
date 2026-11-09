// `include!` splices the target file's items in at the call site, so the
// helper's body is compiled as part of this crate.
//@ mode: run-pass
//@ expect-exit: 41
//@ feature: LANG-MACRO-BUILTIN-INCLUDE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
include!("helper.g");
fn main() -> i32 { helper() }
