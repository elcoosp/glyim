// Multi-file: `mod helper;` pulls in `helper.g` next to this file.
//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SCOPE-MOD-DECL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
mod helper;
fn main() -> i32 { helper::value() }
