// Multi-file with nesting: main includes `outer`, which includes `inner`.
//@ mode: run-pass
//@ expect-exit: 7
//@ feature: LANG-SCOPE-MOD-DECL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
mod outer;
fn main() -> i32 { outer::via_inner() }
