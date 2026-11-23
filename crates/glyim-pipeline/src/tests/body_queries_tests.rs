//! `M6.4` body-query backdating tests (`M2` `DoD` a + b).
//!
//! Proves the early-cutoff contract:
//! * a `thir_body` re-run with a structurally identical body returns the same
//!   `Arc` (backdate) and does NOT invalidate the downstream `mir_build` cache;
//! * a `thir_body` re-run with a changed body returns a fresh `Arc` and DOES
//!   invalidate `mir_build`;
//! * a `mir_build` cache hit does NOT call `compute` (the closure panics on
//!   recompute).
//!
//! The `def_map` invariance property test (`M2` `DoD` c, ≥50 body-only edits) lives
//! in `glyim-test` alongside the `M6.1`/`M6.3` invariance tests — it needs the full
//! parse+expand+`build_def_map` chain.

use crate::body_queries::BodyQueries;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{Database, QueryKind};
use glyim_mir::Body;
use glyim_span::Span;
use glyim_type::Ty;
use glyim_typeck::thir;
use std::sync::Arc;

/// A minimal THIR body (owner + UNIT return + no params/stmts + DUMMY span),
/// distinct per `local` so two bodies with different owners hash differently.
fn thir_body_for(local: u32) -> thir::Body {
    thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(local)),
        params: Vec::new(),
        return_ty: Ty::UNIT,
        stmts: Vec::new(),
        span: Span::DUMMY,
    }
}

/// The `mir_build` cache id the `thir_body` query invalidates for a given def.
/// Mirrors `body_queries::mir_build_id` (private) so the test can inspect the
/// cache directly.
fn mir_build_id(krate: CrateId, local: LocalDefId) -> u64 {
    (u64::from(krate.to_raw()) << 32) | u64::from(local.to_raw())
}

/// `M2` `DoD` (b): a backdated `thir_body` re-run returns the same `Arc` and does
/// NOT invalidate the downstream `mir_build` cache.
#[test]
fn thir_body_backdate_returns_same_arc_and_keeps_mir_build_cache() {
    let db = Database::default();
    let def_id = LocalDefId::from_raw(7);

    // First thir_body: cache miss → insert + invalidate mir_build (no-op, empty).
    let arc1 = db.thir_body_query(def_id, || thir_body_for(7));

    // Simulate a prior mir_build_query by inserting a mir_build entry for this def.
    let mir_id = mir_build_id(CrateId::from_raw(0), def_id);
    let mir_arc = Arc::new(Body::dummy(DefId::new(CrateId::from_raw(0), def_id)));
    db.query_cache_insert(
        QueryKind::MirBuild,
        mir_id,
        glyim_db::body_queries::sentinel_fingerprint(),
        mir_arc.clone(),
    );
    assert!(
        db.query_cache_value(QueryKind::MirBuild, mir_id).is_some(),
        "mir_build cache seeded"
    );

    // Second thir_body with a STRUCTURALLY IDENTICAL body → backdate.
    let arc2 = db.thir_body_query(def_id, || thir_body_for(7));
    assert!(
        Arc::ptr_eq(&arc1, &arc2),
        "a backdated thir_body must return the same Arc (no recompute)"
    );

    // The backdate must NOT have invalidated the downstream mir_build cache.
    assert!(
        db.query_cache_value(QueryKind::MirBuild, mir_id).is_some(),
        "a backdated thir_body must not invalidate the downstream mir_build cache"
    );
}

/// `M2` `DoD` (b) positive control: a `thir_body` re-run with a CHANGED body
/// returns a fresh `Arc` and DOES invalidate the downstream `mir_build` cache.
#[test]
fn thir_body_change_returns_fresh_arc_and_invalidates_mir_build() {
    let db = Database::default();
    let def_id = LocalDefId::from_raw(9);

    // First thir_body (owner local 9).
    let arc1 = db.thir_body_query(def_id, || thir_body_for(9));

    // Seed the mir_build cache for this def.
    let mir_id = mir_build_id(CrateId::from_raw(0), def_id);
    db.query_cache_insert(
        QueryKind::MirBuild,
        mir_id,
        glyim_db::body_queries::sentinel_fingerprint(),
        Arc::new(Body::dummy(DefId::new(CrateId::from_raw(0), def_id))),
    );
    assert!(db.query_cache_value(QueryKind::MirBuild, mir_id).is_some());

    // Second thir_body with a DIFFERENT body (owner local 99 → different Debug →
    // different StableHasher fingerprint) → cache miss → invalidate mir_build.
    let arc2 = db.thir_body_query(def_id, || thir_body_for(99));
    assert!(
        !Arc::ptr_eq(&arc1, &arc2),
        "a changed thir_body must return a fresh Arc"
    );
    assert!(
        db.query_cache_value(QueryKind::MirBuild, mir_id).is_none(),
        "a changed thir_body must invalidate the downstream mir_build cache"
    );
}

/// `M2` `DoD` (a): a `mir_build` cache hit does NOT call `compute` (the closure
/// panics on recompute) and returns the cached `Arc`.
#[test]
fn mir_build_cache_hit_does_not_recompute() {
    let db = Database::default();
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(5));

    // Seed the mir_build cache (simulating a prior mir_build_query that
    // validated + cached the body).
    let mir_id = mir_build_id(owner.krate, owner.local_id);
    let cached_arc = Arc::new(Body::dummy(owner));
    db.query_cache_insert(
        QueryKind::MirBuild,
        mir_id,
        glyim_db::body_queries::sentinel_fingerprint(),
        cached_arc.clone(),
    );

    // Build a frozen TyCtx the validator would read (cache hit skips validation,
    // so any TyCtx — even a fresh test one — suffices).
    let ty_ctx = glyim_test::test_ty_ctx().freeze();

    // mir_build_query must cache-hit and NOT call compute (the closure panics).
    let arc = db
        .mir_build_query(owner, &ty_ctx, || panic!("cache hit must not recompute"))
        .expect("cache hit returns Ok");
    assert!(
        Arc::ptr_eq(&cached_arc, &arc),
        "a mir_build cache hit must return the same Arc"
    );
}
