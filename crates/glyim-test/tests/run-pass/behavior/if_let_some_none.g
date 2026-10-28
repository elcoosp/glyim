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
//
// §3.5.24: the `None` scrutinee is bound to an explicitly-typed local.
// The original form `if let Option::Some(_) = Option::None { … }` has an
// **unconstrained** `Option<_>` — real Rust rejects it with E0282 (type
// annotations needed). glyim's failure mode was worse (silent ICE at
// LLVM codegen instead of a typeck diagnostic), tracked separately in
// §3.5.24. This fixture uses the valid form so the lowering path stays
// covered while that's fixed.
// test-mode: run-pass
// exit-code: 49
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a: i32 = if let Option::Some(x) = Option::Some(42) { x } else { 0 };
    let none_val: Option<i32> = Option::None;
    let b: i32 = if let Option::Some(_) = none_val { 1 } else { 7 };
    // 42 + 7 = 49
    a + b
}
