//! Invariants the builtin-method registry must satisfy at all times.
//!
//! These guard bug classes that produce *silent* wrong code or a late codegen
//! panic rather than a diagnostic:
//!
//! 1. **Parallel-table drift** — a builtin registered in
//!    `builtin_method_fns` / `primitive_method_fns` but not in `fn_sigs`.
//!    Codegen resolves the call ABI via `fn_sig(id)`, so a missing entry
//!    panics with `no signature registered for function FnDefId(N)` long
//!    after the mistake was made. (Observed: `String::new`/`push_str`.)
//!
//! 2. **Sentinel leakage** — a method resolved to a `u32::MAX - k` sentinel
//!    that lowering replaces with an `Error` constant. Nothing rejects the
//!    sentinel at the registry boundary, so the poison travels to codegen.
//!    (Observed: `to_string`.)
//!
//! 3. **Non-unique ids** — two different `(type, method)` entries handed the
//!    same `FnDefId`, so one shadows the other.

use glyim_test::test_ty_ctx;

/// Sentinel slots reserved by `check_expr.rs` / `unify.rs` for
/// compiler-synthetic builtins that have **no** real `FnDef` (`into`,
/// `to_string` pre-fix, `clone`, `drop`). A real builtin must never be
/// assigned one of these.
fn is_sentinel(raw: u32) -> bool {
    raw >= u32::MAX - 4
}

#[test]
fn every_builtin_fn_has_a_signature() {
    let ctx = test_ty_ctx().freeze();
    let ids = ctx.builtin_fn_ids();
    assert!(
        !ids.is_empty(),
        "builtin method registry is EMPTY — population regressed"
    );
    let mut missing = Vec::new();
    for id in &ids {
        if ctx.fn_sig(*id).is_none() {
            missing.push(id.to_raw());
        }
    }
    assert!(
        missing.is_empty(),
        "{} builtin FnDefId(s) are in the method registry but have no \
         `fn_sigs` entry. Codegen panics with `no signature registered`:\n  {:?}",
        missing.len(),
        missing
    );
}

#[test]
fn no_builtin_id_uses_a_sentinel_slot() {
    let ctx = test_ty_ctx().freeze();
    let bad: Vec<u32> = ctx
        .builtin_fn_ids()
        .into_iter()
        .map(|id| id.to_raw())
        .filter(|raw| is_sentinel(*raw))
        .collect();
    assert!(
        bad.is_empty(),
        "builtin registry handed out sentinel FnDefId slot(s) {:?}. Those \
         are reserved for compiler-synthetic methods with no real FnDef and \
         are replaced by an Error constant during lowering — assigning one \
         to a real builtin makes the call silently evaluate to poison.",
        bad
    );
}

#[test]
fn builtin_ids_are_unique() {
    let ctx = test_ty_ctx().freeze();
    let mut ids = ctx.builtin_fn_ids();
    let before = ids.len();
    ids.sort_unstable_by_key(|id| id.to_raw());
    ids.dedup_by_key(|id| id.to_raw());
    assert_eq!(
        before,
        ids.len(),
        "duplicate FnDefId(s) in the builtin registry — two methods share an \
         id, so one silently dispatches to the other's body"
    );
}

#[test]
fn builtin_ids_are_in_the_synthetic_range() {
    // The registry hands out ids from `9_000` up (see `next_builtin_fn_id`).
    // An id below that would collide with a real user/parser FnDefId.
    let ctx = test_ty_ctx().freeze();
    let bad: Vec<u32> = ctx
        .builtin_fn_ids()
        .into_iter()
        .map(|id| id.to_raw())
        .filter(|raw| *raw < 9_000)
        .collect();
    assert!(
        bad.is_empty(),
        "builtin registry handed out id(s) {:?} below the synthetic range \
         (9_000); these collide with real FnDefIds",
        bad
    );
}

#[test]
fn reverse_lookup_agrees_with_the_registry() {
    // `builtin_fn_id(id)` must resolve every id `builtin_fn_ids()` returns.
    let ctx = test_ty_ctx().freeze();
    for id in ctx.builtin_fn_ids() {
        assert!(
            ctx.builtin_fn_id(id).is_some(),
            "builtin_fn_id({}) returned None for an id in the registry — \
             forward and reverse tables disagree",
            id.to_raw()
        );
    }
}
