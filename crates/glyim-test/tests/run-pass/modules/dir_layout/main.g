// Multi-file with `dir/mod.g` layout: `mod math;` resolves to
// `math/mod.g` when `math.g` is absent.
//@ mode: run-pass
//@ expect-exit: 12
//@ feature: LANG-SCOPE-MOD-DECL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
mod math;
fn main() -> i32 { math::triple(4) }
