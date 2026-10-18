// ANTI-REGRESSION: nested `match` whose inner scrutinee is a field
// binding of the outer match.
//
// `let a = Option::Some(Option::Some(5)); match a { Some(inner) =>
// match inner { Some(x) => x, .. }, .. }` failed with "mismatched
// types: T vs i32" / "expected integer type, found T".
//
// `check_pattern` classified `expected_ty` as-written. The inner match
// receives the *pre-unification* type of the `inner` binding — an
// `Infer(?T)` that is already bound to `Option<i32>` in the inference
// table, but not reflected in the local binding. The variant-pattern
// arm therefore fell through to the `expected_ty` fallback and bound
// `x: Param(T)`; the arm body then failed against the outer `i32`.
//
// Fixed by resolving `expected_ty` through `resolve_ty_shallow_preserve_int`
// at `check_pattern` entry. (`preserve_int`, not the plain
// `resolve_ty_shallow`, because the latter defaults unbound int vars
// to `i32` and changed behaviour for already-working pattern sites.)
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a = Option::Some(Option::Some(5));
    match a {
        Option::Some(inner) => match inner {
            Option::Some(x) => x,
            Option::None => -1,
        },
        Option::None => -2,
    }
}
