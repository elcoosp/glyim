//! M6.4 body queries: typed `thir_body`, `mir_build`, and `def_map` accessors
//! on [`Database`] with early-cutoff backdating.
//!
//! The plan (§6 M6.4) lands signature + body queries on the query database so
//! a body-only edit invalidates only the queries that transitively depend on
//! it. This module is the **typed** layer: the type-erased cache storage lives
//! on `Database` in [`glyim_db::body_queries`] (so `glyim-db` does not need to
//! depend on `glyim-typeck`/`glyim-mir`/`glyim-def-map`); these methods
//! downcast the erased values, run the backdating equality check, and handle
//! downstream invalidation.
//!
//! # Backdating contract
//!
//! A `thir_body` re-run computes the body's `StableHasher` fingerprint
//! (M6.1). If it matches the cached entry's fingerprint, the cached `Arc` is
//! returned **unchanged** and the downstream `mir_build` cache for that def is
//! NOT invalidated — so a `mir_build` query that follows is a cache hit. A
//! body edit moves the fingerprint (cache miss → insert + invalidate
//! `mir_build`); an unrelated body edit leaves it untouched (cache hit →
//! backdate). The `def_map` query is keyed on the body-agnostic
//! `item_tree_fingerprint` (M6.3), so a body-only edit is always a cache hit
//! for it — the M6.4 definition of done.
//!
//! # Validator integration (M5.5)
//!
//! `mir_build_query` runs the unified MIR validator
//! ([`glyim_mir::validate::validate_body`]) on every freshly-built body before
//! caching it. A validation failure is surfaced as an `internal_error`
//! diagnostic naming the def; the body is not cached (so a re-query recomputes
//! + re-validates, never serving a known-bad body).

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::{Fingerprint, StableHasher};
use glyim_db::{Database, QueryContext, QueryKind, RebuildOutcome};
use glyim_def_map::CrateDefMap;
use glyim_diag::{CompResult, GlyimDiagnostic, sort_diagnostics};
use glyim_mir::Body;
use glyim_span::FileId;
use glyim_type::TyCtx;
use glyim_typeck::thir;
use std::sync::Arc;

/// M6.4 body/signature queries with early-cutoff backdating. The caching/
/// storage lives on `Database` (type-erased, in
/// [`glyim_db::body_queries`]); these typed methods downcast the erased
/// values, run the backdating equality check, and handle downstream
/// invalidation.
///
/// Consumers should depend on `impl BodyQueries` (or `&dyn BodyQueries` for
/// object-safe calls — note the methods are generic over `compute`, so the
/// trait is not object-safe; call it through the concrete `Database` type).
pub trait BodyQueries {
    /// `thir_body` query: compute (or cache-hit) a THIR body for `def_id`.
    ///
    /// Early cutoff (backdating): if the recomputed body's `StableHasher`
    /// fingerprint matches the cached entry, the cached `Arc` is returned
    /// (byte-identical pointer) and the downstream `mir_build` cache for this
    /// def is NOT invalidated. A body edit moves the fingerprint (cache miss →
    /// insert + invalidate `mir_build`); an unrelated edit leaves it untouched
    /// (cache hit → backdate).
    fn thir_body_query(
        &self,
        def_id: LocalDefId,
        compute: impl FnOnce() -> thir::Body,
    ) -> Arc<thir::Body>;

    /// `mir_build` query: lower a THIR body to MIR, validate it with the
    /// unified MIR validator (M5.5), and cache.
    ///
    /// Cache hit returns the cached `Arc` (the `thir_body` backdating kept it
    /// valid). On a miss, the `compute` closure builds the body, the validator
    /// runs, and the validated body is cached. A validation failure is
    /// surfaced as an `internal_error` diagnostic; the body is not cached.
    fn mir_build_query(
        &self,
        def_id: DefId,
        ty_ctx: &TyCtx,
        compute: impl FnOnce() -> CompResult<Body>,
    ) -> CompResult<Arc<Body>>;

    /// `mir_build` query with a diagnostic accumulator (M6.5): like
    /// [`BodyQueries::mir_build_query`], but the `compute` closure also
    /// returns the non-fatal diagnostics it emitted while building the body.
    /// Those diagnostics are stable-sorted (RG-06), cached with the body,
    /// and replayed verbatim on a cache hit — so a cached run prints
    /// identical diagnostics to a fresh run (the M6.5 definition of done).
    ///
    /// On a cache hit the closure is **not** called: the cached `Arc<Body>`
    /// and the cached `Arc<Vec<GlyimDiagnostic>>` (the same allocation the
    /// first run returned) are handed back. On a miss the body is validated
    /// (M5.5) before caching; a validation failure is surfaced as
    /// `internal_error` diagnostics and the body is not cached.
    fn mir_build_query_with_diags(
        &self,
        def_id: DefId,
        ty_ctx: &TyCtx,
        compute: impl FnOnce() -> CompResult<(Body, Vec<GlyimDiagnostic>)>,
    ) -> CompResult<(Arc<Body>, Arc<Vec<GlyimDiagnostic>>)>;

    /// `def_map` query: compute (or cache-hit) the crate def map.
    ///
    /// The cache key is the body-agnostic `item_tree_fingerprint` (M6.3), so a
    /// body-only edit is a cache hit (early cutoff): the same `Arc<CrateDefMap>`
    /// is returned (`Arc::ptr_eq` holds). A signature change (renaming/adding/
    /// removing an item) moves the fingerprint → cache miss → recompute.
    fn def_map_query(
        &self,
        file: FileId,
        compute: impl FnOnce() -> CrateDefMap,
    ) -> Arc<CrateDefMap>;
}

impl BodyQueries for Database {
    fn thir_body_query(
        &self,
        def_id: LocalDefId,
        compute: impl FnOnce() -> thir::Body,
    ) -> Arc<thir::Body> {
        let id = u64::from(def_id.to_raw());
        let body = compute();
        let hash = thir_body_fingerprint(&body);
        let cached_hash = self.query_cache_hash(QueryKind::ThirBody, id);
        // Backdating: hash match → return cached Arc, NO downstream invalidation.
        if cached_hash == Some(hash)
            && let Some(cached) = self.query_cache_value(QueryKind::ThirBody, id)
            && let Ok(arc) = cached.downcast::<thir::Body>()
        {
            // M6.9: record the backdate so --explain-rebuild can show this
            // query did not recompute its downstream `mir_build`.
            self.record_rebuild(QueryKind::ThirBody, id, RebuildOutcome::Backdated);
            // M6.8: when the verify seam is enabled, record `Verified` — the
            // backdate recomputed the fingerprint and it matched the cached
            // entry's hash, so the cached `Arc<thir::Body>` is what a clean
            // recompute would produce. Zero-overhead when the env var is unset
            // (the snapshot stays empty).
            if self.incr_verify_enabled() {
                self.record_incr_verify(
                    QueryKind::ThirBody,
                    id,
                    glyim_db::IncrVerifyOutcome::Verified,
                );
            }
            return arc;
        }
        // M6.8: when the verify seam is enabled AND a cached entry existed but
        // its hash no longer matches the recomputed fingerprint, record
        // `Mismatch` before re-caching — the cached value was stale relative
        // to what `compute` produces now (a backdating bug or cache poisoning).
        // This is the load-bearing correctness check the nightly 10k-sequence
        // harness (M2) fails CI on (plan M6.8 `DoD`: "0 disagreements").
        if self.incr_verify_enabled() && cached_hash.is_some() && cached_hash != Some(hash) {
            self.record_incr_verify(
                QueryKind::ThirBody,
                id,
                glyim_db::IncrVerifyOutcome::Mismatch,
            );
        }
        // Miss or content changed: cache new body + invalidate downstream mir_build.
        let arc = Arc::new(body);
        self.query_cache_insert(QueryKind::ThirBody, id, hash, arc.clone());
        self.query_cache_invalidate(
            QueryKind::MirBuild,
            mir_build_id(QueryContext::krate(self), def_id),
        );
        // M6.9: record the miss (fresh content cached + downstream invalidated).
        self.record_rebuild(QueryKind::ThirBody, id, RebuildOutcome::Miss);
        arc
    }

    fn mir_build_query(
        &self,
        def_id: DefId,
        ty_ctx: &TyCtx,
        compute: impl FnOnce() -> CompResult<Body>,
    ) -> CompResult<Arc<Body>> {
        // Delegate to the M6.5 accumulator variant with an empty diagnostic
        // vector — a `mir_build` that emits no diagnostics is the common case,
        // and the delegation keeps the two entry points from diverging.
        let (arc, _diags) = self.mir_build_query_with_diags(def_id, ty_ctx, || {
            compute().map(|body| (body, Vec::new()))
        })?;
        Ok(arc)
    }

    fn mir_build_query_with_diags(
        &self,
        def_id: DefId,
        ty_ctx: &TyCtx,
        compute: impl FnOnce() -> CompResult<(Body, Vec<GlyimDiagnostic>)>,
    ) -> CompResult<(Arc<Body>, Arc<Vec<GlyimDiagnostic>>)> {
        let id = mir_build_id_from_def(def_id);
        // Cache hit? (thir_body was backdated → mir_build still valid.)
        // Replay the cached diagnostics verbatim (M6.5): a cached run prints
        // identical diagnostics to a fresh run, with no recompute.
        if let Some(cached) = self.query_cache_value(QueryKind::MirBuild, id)
            && let Ok(arc) = cached.downcast::<Body>()
            && let Some(diags) = self.query_cache_diagnostics(QueryKind::MirBuild, id)
        {
            // M6.9: record the hit (compute did not run — the entry was present).
            self.record_rebuild(QueryKind::MirBuild, id, RebuildOutcome::Hit);
            // M6.8: when the verify seam is enabled, re-run `compute`, validate
            // the fresh body, and compare its `StableHasher` fingerprint to the
            // cached `Arc<Body>`'s fingerprint. A match records `Verified`
            // (the upstream `thir_body` backdate was correct); a mismatch
            // records `Mismatch` (the cached body is stale — a backdating bug
            // the nightly 10k-sequence harness fails CI on). The cached value
            // is still returned (the seam is observational, not corrective),
            // so a verify-enabled run produces identical diagnostics to a
            // verify-disabled run.
            if self.incr_verify_enabled() {
                let outcome = match compute() {
                    Ok((fresh, _)) => {
                        // Validate the fresh body so a verify re-run cannot
                        // mask an M5.5 validator regression — a fresh body
                        // that fails validation is a Mismatch by definition.
                        if glyim_mir::validate::validate_body(ty_ctx, &fresh).is_err() {
                            glyim_db::IncrVerifyOutcome::Mismatch
                        } else {
                            let fresh_hash = mir_body_fingerprint(&fresh);
                            let cached_hash = mir_body_fingerprint(&arc);
                            if fresh_hash == cached_hash {
                                glyim_db::IncrVerifyOutcome::Verified
                            } else {
                                glyim_db::IncrVerifyOutcome::Mismatch
                            }
                        }
                    }
                    // A fresh `compute` that now errors is a Mismatch: the
                    // cached entry claims a successful compute that a clean
                    // re-run does not reproduce.
                    Err(_) => glyim_db::IncrVerifyOutcome::Mismatch,
                };
                self.record_incr_verify(QueryKind::MirBuild, id, outcome);
            }
            return Ok((arc, diags));
        }
        // Miss: compute + validate (M5.5) + cache with diagnostics (M6.5).
        let (body, mut diags) = compute()?;
        glyim_mir::validate::validate_body(ty_ctx, &body).map_err(|violations| {
            violations
                .into_iter()
                .map(|v| {
                    GlyimDiagnostic::internal_error(format!(
                        "MIR validation failed for {def_id:?}: {v}"
                    ))
                })
                .collect::<Vec<GlyimDiagnostic>>()
        })?;
        // Stable-sort the accumulator (RG-06) so cached and fresh runs agree
        // byte-for-byte regardless of the order `compute` emitted diagnostics.
        sort_diagnostics(&mut diags);
        let diags = Arc::new(diags);
        let arc = Arc::new(body);
        // mir_build's backdating is keyed on thir_body's hash (upstream), so the
        // entry stores the sentinel fingerprint — its presence/absence is the
        // cache check, not its hash.
        self.query_cache_insert_with_diags(
            QueryKind::MirBuild,
            id,
            glyim_db::body_queries::sentinel_fingerprint(),
            arc.clone(),
            diags.clone(),
        );
        // M6.9: record the miss (compute + validate ran; fresh body cached).
        self.record_rebuild(QueryKind::MirBuild, id, RebuildOutcome::Miss);
        Ok((arc, diags))
    }

    fn def_map_query(
        &self,
        file: FileId,
        compute: impl FnOnce() -> CrateDefMap,
    ) -> Arc<CrateDefMap> {
        let id = u64::from(file.to_raw());
        let fp = QueryContext::item_tree_fingerprint(self, file);
        // Backdating: fingerprint match → return cached Arc (body-only edit → hit).
        if self.query_cache_hash(QueryKind::DefMap, id) == Some(fp)
            && let Some(cached) = self.query_cache_value(QueryKind::DefMap, id)
            && let Ok(arc) = cached.downcast::<CrateDefMap>()
        {
            // M6.9: record the backdate (body-only edit → fingerprint unchanged).
            self.record_rebuild(QueryKind::DefMap, id, RebuildOutcome::Backdated);
            return arc;
        }
        let arc = Arc::new(compute());
        self.query_cache_insert(QueryKind::DefMap, id, fp, arc.clone());
        // M6.9: record the miss (signature change → fingerprint moved → recompute).
        self.record_rebuild(QueryKind::DefMap, id, RebuildOutcome::Miss);
        arc
    }
}

/// The `StableHasher` fingerprint of a THIR body (M6.4 backdating equality
/// check). Hashes the body's `Debug` rendering — derived `Debug` is
/// deterministic for a given body value, so two structurally identical bodies
/// hash identically. The hash is the sole backdating key: a body edit moves it
/// (cache miss → invalidate downstream `mir_build`); an unrelated body edit
/// leaves it untouched (cache hit → backdate).
fn thir_body_fingerprint(body: &thir::Body) -> Fingerprint {
    let mut h = StableHasher::new();
    h.write_str(&format!("{body:?}"));
    Fingerprint::from_bytes(h.finish())
}

/// The `StableHasher` fingerprint of a MIR `Body` (M6.8 verify-seam equality
/// check). Mirrors [`thir_body_fingerprint`]: hashes the `Debug` rendering so
/// two structurally identical bodies hash identically. Used only on the
/// `GLYIM_INCR_VERIFY=1` `mir_build` cache-hit path to confirm the cached
/// `Arc<Body>` is what a clean recompute would produce — `mir_build`'s
/// backdating is keyed on `thir_body`'s hash (upstream), so the body's own
/// fingerprint is the verify comparison, never the cache key.
fn mir_body_fingerprint(body: &Body) -> Fingerprint {
    let mut h = StableHasher::new();
    h.write_str(&format!("{body:?}"));
    Fingerprint::from_bytes(h.finish())
}

/// The `mir_build` cache id for `(krate, local_def_id)`. Combines the two raw
/// ids into a single `u64` key (krate in the high 32 bits, local in the low
/// 32) so the `thir_body` query can invalidate the downstream `mir_build`
/// entry by id without a separate lookup.
fn mir_build_id(krate: CrateId, def_id: LocalDefId) -> u64 {
    (u64::from(krate.to_raw()) << 32) | u64::from(def_id.to_raw())
}

/// `mir_build_id` from a full `DefId` (the `mir_build_query` call site).
fn mir_build_id_from_def(def_id: DefId) -> u64 {
    mir_build_id(def_id.krate, def_id.local_id)
}
