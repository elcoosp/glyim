//! M6.8 incremental ≡ clean oracle: `GLYIM_INCR_VERIFY` recompute-and-compare
//! seam tests.
//!
//! Plan §6 M6.8 `DoD`: "`GLYIM_INCR_VERIFY=1` re-computes a sample of cache
//! hits and compares fingerprints". The seam is the verification layer over
//! the M6.4/M6.5 query cache: when enabled (the env var at `Database::new`,
//! or the per-`Database` test override), a cache-serving path (`Backdated` or
//! `Hit`) re-runs `compute`, recomputes the content `Fingerprint`, and
//! compares it to the cached entry's hash. A match records `Verified`; a
//! mismatch records `Mismatch` — a bug in the backdating logic (the cached
//! value is stale relative to what `compute` would produce now). The report
//! is the consumer view: `--explain-rebuild` and the nightly 10k-sequence
//! harness (M2) read it to assert "0 disagreements".
//!
//! These tests pin the seam: the per-`Database` flag gates it, the report
//! records both outcomes, and a deliberately-stale cache entry is caught as
//! a `Mismatch`. The flag is flipped via `Database::set_incr_verify_for_test`
//! (not process-global `set_var`) so the parallel test runner does not race
//! on env mutation — edition 2024 made `std::env::set_var` unsafe.

use crate::body_queries::BodyQueries;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{Database, IncrVerifyOutcome, QueryKind};
use glyim_mir::Body;
use glyim_span::Span;
use glyim_type::Ty;
use glyim_typeck::thir;

/// A minimal THIR body distinct per `local` (different owner → different
/// `Debug` → different fingerprint).
fn thir_body_for(local: u32) -> thir::Body {
    thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(local)),
        params: Vec::new(),
        return_ty: Ty::UNIT,
        stmts: Vec::new(),
        span: Span::DUMMY,
    }
}

/// When the M6.8 verify seam is enabled on the `Database`, a
/// `thir_body_query` backdate path records `Verified` — the backdate check
/// (recomputed hash == cached hash) is itself the verification, so the seam
/// records the outcome.
///
/// Plan §6 M6.8: "re-computes a sample of cache hits and compares
/// fingerprints" — the backdate recomputes the fingerprint and compares it to
/// the cached entry's hash; the verify seam records the conclusion.
#[test]
fn thir_body_backdate_records_verified_when_enabled() {
    let db = Database::default();
    db.set_incr_verify_for_test(true);
    let def_id = LocalDefId::from_raw(31);

    // Fresh: cache miss → compute + cache + record Miss. Verify does not run
    // on a miss (there is no cached entry to compare against).
    db.thir_body_query(def_id, || thir_body_for(31));
    let report = db.incr_verify_snapshot();
    assert_eq!(report.len(), 0, "verify does not run on a fresh miss");

    // Re-run with a structurally identical body → backdate. The backdate
    // recomputed the fingerprint and it matched the cached hash → Verified.
    db.clear_incr_verify();
    db.thir_body_query(def_id, || thir_body_for(31));
    let report = db.incr_verify_snapshot();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].kind, QueryKind::ThirBody);
    assert_eq!(report[0].outcome, IncrVerifyOutcome::Verified);
}

/// When the verify seam is disabled (the default), the backdate path does NOT
/// record a verify event — the seam is zero-overhead when disabled. This is
/// the M6.8 hot-path guarantee: the oracle is opt-in.
#[test]
fn thir_body_backdate_skips_verify_when_disabled() {
    let db = Database::default();
    db.set_incr_verify_for_test(false);
    let def_id = LocalDefId::from_raw(32);

    db.thir_body_query(def_id, || thir_body_for(32));
    db.clear_incr_verify();

    // Re-run: backdate, verify disabled → no record.
    db.thir_body_query(def_id, || thir_body_for(32));
    let report = db.incr_verify_snapshot();
    assert_eq!(report.len(), 0, "verify disabled: report stays empty");
}

/// When the verify seam is enabled and the cached entry's hash does NOT match
/// the recomputed fingerprint, the seam records `Mismatch` — the load-bearing
/// correctness check. This simulates a backdating bug: the cache holds a
/// stale fingerprint relative to what `compute` produces now.
///
/// Plan §6 M6.8 `DoD`: "10k sequences nightly, 0 disagreements" — a
/// `Mismatch` is exactly a disagreement, and the nightly harness (M2) fails
/// CI on any non-zero count.
#[test]
fn thir_body_backdate_records_mismatch_on_stale_cache() {
    let db = Database::default();
    db.set_incr_verify_for_test(true);
    let def_id = LocalDefId::from_raw(33);

    // Seed the cache with a body whose fingerprint is for `local=33`...
    db.thir_body_query(def_id, || thir_body_for(33));

    // ...then corrupt the cached hash so it can no longer match the
    // recomputed fingerprint (simulating a backdating bug or a cache
    // poisoning). The verify seam must catch this.
    let stale = glyim_core::Fingerprint::from_bytes([0xFF; 16]);
    db.overwrite_query_cache_hash_for_test(QueryKind::ThirBody, u64::from(def_id.to_raw()), stale);

    db.clear_incr_verify();
    // Re-run with the structurally identical body. The backdate recomputes
    // the fingerprint; the cached hash no longer matches → the query falls
    // through to a Miss (re-cache), but the verify seam records the
    // `Mismatch` first (the cached entry was stale).
    db.thir_body_query(def_id, || thir_body_for(33));
    let report = db.incr_verify_snapshot();
    assert!(
        report
            .iter()
            .any(|e| e.outcome == IncrVerifyOutcome::Mismatch),
        "stale cache must record a Mismatch: {report:?}"
    );
    assert_eq!(db.incr_verify_mismatch_count(), 1);
}

/// `mir_build_query_with_diags` `Hit` path: when the verify seam is enabled,
/// a cache hit re-runs `compute` and records `Verified` when the recomputed
/// body's fingerprint matches the cached entry.
///
/// `mir_build`'s backdating is keyed on `thir_body`'s hash (upstream), so
/// the verify seam confirms the cached `Body` is still the one `compute`
/// would produce — i.e. the upstream `thir_body` backdate was correct.
#[test]
fn mir_build_hit_records_verified_when_enabled() {
    let db = Database::default();
    db.set_incr_verify_for_test(true);
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(34));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();

    // Fresh: cache miss → compute + validate + cache + Miss.
    db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((Body::dummy(owner), Vec::new())))
        .expect("compute succeeds");
    let report = db.incr_verify_snapshot();
    assert_eq!(report.len(), 0, "verify does not run on a fresh miss");

    // Re-run: cache hit. Verify re-runs `compute` and records `Verified`.
    db.clear_incr_verify();
    db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((Body::dummy(owner), Vec::new())))
        .expect("cache hit returns Ok");
    let report = db.incr_verify_snapshot();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].kind, QueryKind::MirBuild);
    assert_eq!(report[0].outcome, IncrVerifyOutcome::Verified);
}

/// `mir_build_query_with_diags` `Hit` path records `Mismatch` when the
/// cached `Body`'s fingerprint no longer matches what `compute` produces now
/// (the upstream `thir_body` backdate was wrong, or the cache was poisoned).
///
/// This pins the M6.8 oracle's pass/fail signal on the `mir_build` family: a
/// non-zero `incr_verify_mismatch_count` is the disagreement the nightly 10k-
/// sequence harness (M2) fails CI on.
#[test]
fn mir_build_hit_records_mismatch_on_diverging_compute() {
    let db = Database::default();
    db.set_incr_verify_for_test(true);
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(35));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();

    // Seed the cache with one body shape (one local).
    db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((Body::dummy(owner), Vec::new())))
        .expect("seed compute succeeds");

    // Re-run with a DIFFERENT body shape (two locals instead of one). The
    // cache hit path returns the cached (one-local) body, but the verify seam
    // re-runs `compute` and finds the fresh body's fingerprint no longer
    // matches → `Mismatch`.
    db.clear_incr_verify();
    db.mir_build_query_with_diags(owner, &ty_ctx, || {
        Ok((body_with_extra_local(owner), Vec::new()))
    })
    .expect("cache hit still returns Ok (seam is observational)");
    let report = db.incr_verify_snapshot();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].kind, QueryKind::MirBuild);
    assert_eq!(report[0].outcome, IncrVerifyOutcome::Mismatch);
    assert_eq!(db.incr_verify_mismatch_count(), 1);
}

/// `incr_verify_mismatch_count` is the single number the nightly 10k-sequence
/// harness (M2) asserts is `0`. A non-zero count fails CI (plan M6.8 `DoD`:
/// "0 disagreements").
#[test]
fn mismatch_count_is_the_oracle_pass_fail_signal() {
    let db = Database::default();
    db.record_incr_verify(QueryKind::ThirBody, 1, IncrVerifyOutcome::Verified);
    db.record_incr_verify(QueryKind::ThirBody, 2, IncrVerifyOutcome::Mismatch);
    db.record_incr_verify(QueryKind::MirBuild, 3, IncrVerifyOutcome::Mismatch);
    assert_eq!(db.incr_verify_mismatch_count(), 2);

    db.clear_incr_verify();
    assert_eq!(db.incr_verify_mismatch_count(), 0);
}

/// A `Body` with two locals (distinct from [`Body::dummy`]'s one-local shape)
/// so its `Debug` fingerprint differs. Used by the `mir_build` mismatch test
/// to simulate a diverging `compute` closure — the cached body is one shape,
/// the re-run produces another.
fn body_with_extra_local(owner: DefId) -> Body {
    let mut body = Body::dummy(owner);
    body.locals.push(glyim_mir::LocalDecl {
        ty: Ty::UNIT,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: glyim_mir::SourceInfo::new(Span::DUMMY),
    });
    body
}
