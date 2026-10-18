// Both a `static` and a `const` in the same program, read side by side,
// to verify the shared `LocalDefId` raw-id-space aliasing doesn't
// cross-read between them. (Static is named STATIC_VAL, not a single
// letter, to sidestep a pre-existing stdlib name-collision bug where
// `const B: i32 = N;` emits a spurious `[X0000] Err expression in THIR`
// warning on the pristine tree — unrelated to the statics feature.)
// test-mode: run-pass
// exit-code: 47
// compile-flags: --with-stdlib
const CONST_VAL: i32 = 11;
static STATIC_VAL: i32 = 19;
fn main() -> i32 { CONST_VAL + STATIC_VAL + 17 }
