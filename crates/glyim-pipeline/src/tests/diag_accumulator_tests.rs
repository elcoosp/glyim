//! M6.5 diagnostic-accumulator tests.
//!
//! Plan §6 M6.5: "Diagnostics as accumulator outputs replayed on cache hit,
//! stable order" — `DoD`: "cached run prints identical diagnostics".
//!
//! A body query whose `compute` closure emits non-fatal diagnostics must
//! cache those diagnostics alongside its value and replay them — in stable
//! order — on a cache hit, so a cached run produces byte-identical
//! diagnostics to a fresh run. Before M6.5 the [`BodyQueries`] cache stored
//! only the value; a cache hit dropped the diagnostics the compute pass had
//! emitted, so the second run of an unchanged body silently diverged from
//! the first. These tests pin the plan's acceptance criterion directly:
//! the second (cache-hit) call must return the same `Arc` **and** the same
//! diagnostics, without re-running `compute`.

use crate::body_queries::BodyQueries;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{Database, QueryKind};
use glyim_diag::GlyimDiagnostic;
use glyim_mir::Body;
use glyim_span::Span;
use std::sync::Arc;

/// A valid (M5.5-validator-passing) dummy MIR body for `owner`: one block
/// with an `Unreachable` terminator and a single `UNIT` return place, no
/// args/stmts/drops. `validate_body` accepts it (no CFG targets, no
/// undeclared locals, no drop glue, no `Subslice`).
fn dummy_body(owner: DefId) -> Body {
    Body::dummy(owner)
}

/// The `mir_build` cache id for `(krate, local)`. Mirrors
/// `body_queries::mir_build_id` (private) so the tests can inspect the
/// cache directly.
fn mir_build_id(krate: CrateId, local: LocalDefId) -> u64 {
    (u64::from(krate.to_raw()) << 32) | u64::from(local.to_raw())
}

/// Plan M6.5 `DoD`: "cached run prints identical diagnostics".
///
/// The first `mir_build_query_with_diags` call is a cache miss: `compute`
/// runs, emits two warnings out of stable order, the body is validated and
/// cached **with** the (stable-sorted) diagnostics. The second call is a
/// cache hit: `compute` must NOT run (the closure panics) and the replayed
/// diagnostics must equal the first run's, element-for-element.
#[test]
fn mir_build_cache_hit_replays_identical_diagnostics() {
    let db = Database::default();
    let owner = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(11));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();

    let warn_a = GlyimDiagnostic::warning(Span::DUMMY, "unused local a");
    let warn_b = GlyimDiagnostic::warning(Span::DUMMY, "unused local b");

    // First call: cache miss → compute emits diagnostics out of stable order.
    let (arc1, diags1) = db
        .mir_build_query_with_diags(owner, &ty_ctx, || {
            Ok((dummy_body(owner), vec![warn_b.clone(), warn_a.clone()]))
        })
        .expect("first compute succeeds and validates");

    // Second call: cache hit → compute must NOT run; diagnostics replayed.
    let (arc2, diags2) = db
        .mir_build_query_with_diags(owner, &ty_ctx, || panic!("cache hit must not recompute"))
        .expect("cache hit returns Ok");

    assert!(
        Arc::ptr_eq(&arc1, &arc2),
        "a cache hit must return the same Arc"
    );
    // M6.5 DoD: "cached run prints identical diagnostics". The cache hit
    // returns the *same allocation* the first run cached (Arc::ptr_eq), so the
    // replayed diagnostics are byte-identical — not a re-computed copy.
    assert!(
        Arc::ptr_eq(&diags1, &diags2),
        "cached run prints identical diagnostics (M6.5 DoD): same Arc"
    );
    assert_eq!(diags1.len(), 2, "both warnings survived the accumulator");
    assert_eq!(
        diags1[0].message, "unused local a",
        "stable sort puts a first"
    );
    assert_eq!(
        diags1[1].message, "unused local b",
        "stable sort puts b second"
    );
}

/// Stable order (RG-06): the accumulator materializes diagnostics sorted by
/// `(file, span, code, severity, message)` regardless of the order `compute`
/// emitted them, so cached and fresh runs agree byte-for-byte even when a
/// pass emits diagnostics in traversal order rather than source order.
#[test]
fn accumulator_materializes_diagnostics_in_stable_order() {
    let db = Database::default();
    let owner = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(12));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();

    // Emit in reverse message order; the accumulator must sort to (a, b).
    let (arc, diags) = db
        .mir_build_query_with_diags(owner, &ty_ctx, || {
            Ok((
                dummy_body(owner),
                vec![
                    GlyimDiagnostic::warning(Span::DUMMY, "unused local b"),
                    GlyimDiagnostic::warning(Span::DUMMY, "unused local a"),
                ],
            ))
        })
        .expect("compute succeeds");

    let _ = arc;
    assert_eq!(diags.len(), 2, "both warnings survive the accumulator");
    assert_eq!(
        diags[0].message, "unused local a",
        "stable sort puts a first"
    );
    assert_eq!(
        diags[1].message, "unused local b",
        "stable sort puts b second"
    );
}

/// The diagnostics are cached with the value: a direct read of the cache
/// entry's diagnostics (via the storage accessor) returns the same `Arc` the
/// typed query returned, proving the accumulator is the cache entry — not a
/// transient side channel that evaporates after the call.
#[test]
fn cached_diagnostics_are_persisted_in_the_storage_entry() {
    let db = Database::default();
    let owner = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(13));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();

    let (_arc, diags) = db
        .mir_build_query_with_diags(owner, &ty_ctx, || {
            Ok((
                dummy_body(owner),
                vec![GlyimDiagnostic::warning(Span::DUMMY, "persisted warning")],
            ))
        })
        .expect("compute succeeds");

    let id = mir_build_id(owner.krate, owner.local_id);
    let cached = db
        .query_cache_diagnostics(QueryKind::MirBuild, id)
        .expect("diagnostics persisted in the cache entry");
    assert!(
        Arc::ptr_eq(&diags, &cached),
        "the typed query's diagnostics Arc is the cached entry's diagnostics Arc"
    );
}

/// A body edit (a `thir_body` change invalidating `mir_build`) re-runs
/// `compute` and re-emits its diagnostics: the accumulator is invalidated
/// with the value, never serving a stale diagnostic set from a prior body.
#[test]
fn body_invalidation_clears_the_accumulator() {
    let db = Database::default();
    let owner = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(14));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();
    let id = mir_build_id(owner.krate, owner.local_id);

    // Seed the mir_build cache with a first diagnostic set.
    db.mir_build_query_with_diags(owner, &ty_ctx, || {
        Ok((
            dummy_body(owner),
            vec![GlyimDiagnostic::warning(Span::DUMMY, "first run warning")],
        ))
    })
    .expect("first compute succeeds");
    assert!(
        db.query_cache_diagnostics(QueryKind::MirBuild, id)
            .is_some(),
        "first run cached its diagnostics"
    );

    // Simulate the upstream `thir_body` invalidating `mir_build` (the
    // `thir_body_query` path does exactly this when a body's fingerprint
    // moves). The accumulator must be cleared with the value.
    db.query_cache_invalidate(QueryKind::MirBuild, id);
    assert!(
        db.query_cache_diagnostics(QueryKind::MirBuild, id)
            .is_none(),
        "invalidation clears the accumulator with the value"
    );

    // A re-query repopulates it with the fresh diagnostic set.
    let (_arc, diags) = db
        .mir_build_query_with_diags(owner, &ty_ctx, || {
            Ok((
                dummy_body(owner),
                vec![GlyimDiagnostic::warning(Span::DUMMY, "second run warning")],
            ))
        })
        .expect("re-query succeeds");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].message, "second run warning");
}
