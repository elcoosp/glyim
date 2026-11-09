// Both a `static` and a `const` in the same program, read side by side,
// to verify the shared `LocalDefId` raw-id-space aliasing doesn't
// cross-read between them. (Static is named STATIC_VAL, not a single
// letter, to sidestep a pre-existing stdlib name-collision bug where
// `const B: i32 = N;` emits a spurious `[X0000] Err expression in THIR`
// warning on the pristine tree — unrelated to the statics feature.)
//@ mode: run-pass
//@ expect-exit: 47
//@ compile-flags: --with-stdlib
//@ feature: LANG-SCOPE-STATIC-ITEM, LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
const CONST_VAL: i32 = 11;
static STATIC_VAL: i32 = 19;
fn main() -> i32 { CONST_VAL + STATIC_VAL + 17 }
