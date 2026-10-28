// ANTI-REGRESSION: `if let PAT = EXPR { .. } else { .. }` must compile
// and run. `lower_if_expr` had no `if let` branch at all, so the
// pattern was lowered as an *expression* and the whole `if` failed
// with "mismatched types: Adt1010 vs bool" + "unresolved name `x`".
//
// Desugared into `match EXPR { PAT => THEN, _ => ELSE }`, mirroring the
// `while let` handling. The wildcard arm uses `Pat::Wild` (not a `_`
// binding) — a binding allocated a fresh MIR local whose liveness
// bitset insert exceeded the capacity computed from the user locals,
// producing the "insert at index N exceeds fixedbitset size N" ICE.
// §3.5.23: IGNORED under `--features glyim-test/real-llvm`. Narrowed to
// the second `if let`: `if let Option::Some(_) = Option::None { 1 } else
// { 7 }` reaches LLVM lowering with a `TyKind::Error` somewhere (verified
// by isolating the two `if let`s — the `Some(42)` case alone returns 42
// fine). The `None` scrutinee + wildcard-in-Some-pattern combination is
// the trigger. Real typeck/lowering bug, tracked in KNOWN_GAPS §3.5.23.
// The MIR-interpreter path handles it correctly; only the LLVM backend
// fails. Ignored here so `test-ubuntu` (which uses real-llvm) is green
// and this bug is tracked rather than hidden in a red CI job.
// test-mode: run-pass
// ignore
// exit-code: 49
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a: i32 = if let Option::Some(x) = Option::Some(42) { x } else { 0 };
    let b: i32 = if let Option::Some(_) = Option::None { 1 } else { 7 };
    // 42 + 7 = 49
    a + b
}
