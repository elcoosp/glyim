// GAP-0009 strict-xfail witness (ledger/gaps/known/0009-const-prop-div-zero.toml):
// the class-b silent divergence the M4.8 per-operator oracle's probe
// sweep found in the optimizer's constant-propagation pass.
//
// `5 / 0` panics at runtime (exit 101 through the interpreter at `-O0`),
// but at `-O2` the const-prop pass folds the division to `0`
// (`crates/glyim-opt/src/constant_prop.rs`,
// `checked_div(..).unwrap_or(0)` — contradicting its own comment), so
// `main` returns the folded `0` and the process exits 0. The witness
// pins the SILENT outcome: `--opt-level=2`, `expect-exit: 0`.
//
// The strict-xfail contract (plan §3.3, M1.6):
//   - while GAP-0009 is open, const-prop keeps folding the division by
//     zero, `main` exits 0, and the pinned run holds (the witnessed
//     debt, not a release blocker — the differential and metamorphic
//     gates skip registered witnesses);
//   - when const-prop stops folding the division by zero (the fix its
//     own comment already describes: fold only when `checked_div` /
//     `checked_rem` return `Some`), the program panics at `-O2`, the
//     pinned run breaks, and this fixture FAILS with
//     `known-gap GAP-0009 closed` — retire the witness in the same
//     change.
//@ mode: run-pass
//@ expect-exit: 0
//@ feature: COMPILER-OPT-CONST-PROP, TOOL-TEST-RUN-PASS
//@ backends: gam
//@ compile-flags: --opt-level=2
//@ known-gap: GAP-0009
fn main() -> i32 {
    let c = 5 / 0;
    return c;
}
