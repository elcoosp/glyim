//! M6.8 incremental ≡ clean oracle: `GLYIM_INCR_VERIFY` recompute-and-compare
//! seam storage.
//!
//! The plan (§6 M6.8, `DoD`: "`GLYIM_INCR_VERIFY=1` re-computes a sample of
//! cache hits and compares fingerprints") layers a verification pass over the
//! M6.4/M6.5 query cache. When the env var is set, a `BodyQueries` `Hit` or
//! `Backdated` path re-runs its `compute` closure, recomputes the content
//! `Fingerprint`, and compares it to the cached entry's hash. A match records
//! `Verified`; a mismatch records `Mismatch` — a disagreement between the
//! cached value and what a clean recompute would produce, i.e. a backdating
//! bug or cache poisoning.
//!
//! The seam is opt-in and zero-overhead when disabled: `incr_verify_enabled()`
//! is a single `getenv` on the cold path, and the `compute` closure is only
//! re-invoked when the env var is set. The nightly 10k-sequence harness (the
//! M6.8 second half) reads the report and fails CI on any non-zero
//! `Mismatch` count.
//!
//! # Determinism
//!
//! The snapshot is sorted by `(QueryKind, id)` before rendering, so two runs
//! over the same compile produce byte-identical output regardless of the
//! order queries were issued. No clock, PRNG, or `HashMap` iteration reaches
//! the report (RG-04 / RG-06).

use parking_lot::RwLock;
use std::sync::atomic::Ordering;

use crate::body_queries::QueryKind;

/// The env-var name that gates the M6.8 verify seam.
pub const GLYIM_INCR_VERIFY: &str = "GLYIM_INCR_VERIFY";

/// Whether the M6.8 `GLYIM_INCR_VERIFY` recompute-and-compare seam is enabled.
///
/// Enabled iff `GLYIM_INCR_VERIFY=1` is set in the process environment. Any
/// other value (including `0`, `true`, `yes`) disables the seam — the
/// `=1`-only gate avoids the foot-gun where `GLYIM_INCR_VERIFY=0` silently
/// enables verification on a machine that set the var to "disable" it. The
/// env var is read fresh per call so tests can toggle it without process
/// boundaries (the same pattern `GLYIM_INTERP_TRACE` / `GLYIM_TMP_FILT` use).
///
/// Returns `false` on any read error (treat a malformed env as "off").
#[must_use]
pub fn incr_verify_enabled() -> bool {
    incr_verify_enabled_for(std::env::var(GLYIM_INCR_VERIFY).as_deref().ok())
}

/// The pure logic of [`incr_verify_enabled`], factored out so the `=1`-only
/// gate can be property-tested without mutating the process environment
/// (env mutation is unsafe under edition 2024 and races the parallel test
/// runner). Returns `true` iff `value == Some("1")`.
#[must_use]
pub fn incr_verify_enabled_for(value: Option<&str>) -> bool {
    value == Some("1")
}

/// The outcome a single `BodyQueries` verify check recorded for one cache-hit
/// call. The two values are the exhaustive set of things the verify seam can
/// conclude: the recomputed fingerprint matched the cached entry's hash
/// (`Verified`), or it did not (`Mismatch` — a disagreement the nightly
/// harness fails CI on).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IncrVerifyOutcome {
    /// The recomputed content `Fingerprint` matched the cached entry's hash.
    /// The cached `Arc<T>` is what a clean recompute would produce — the
    /// backdating logic is correct for this query.
    Verified,
    /// The recomputed `Fingerprint` did NOT match the cached entry's hash.
    /// The cached value is stale relative to what `compute` would produce now
    /// — a backdating bug or cache poisoning. The nightly 10k-sequence
    /// harness fails CI on any non-zero `Mismatch` count (plan M6.8 `DoD`:
    /// "0 disagreements").
    Mismatch,
}

impl IncrVerifyOutcome {
    /// The lowercase token used in the `--explain-rebuild` verify column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            IncrVerifyOutcome::Verified => "verified",
            IncrVerifyOutcome::Mismatch => "mismatch",
        }
    }
}

/// A single recorded M6.8 verify event: which query family, which cache id,
/// and whether the recomputed fingerprint matched the cached entry's hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IncrVerifyEvent {
    /// The query family (`thir_body` / `mir_build` / `def_map`).
    pub kind: QueryKind,
    /// The cache id the query was keyed on (local def id for `thir_body`,
    /// `(krate, local)` packed for `mir_build`, `FileId` for `def_map`).
    pub id: u64,
    /// Whether the recomputed fingerprint matched the cached entry's hash.
    pub outcome: IncrVerifyOutcome,
}

/// The M6.8 verify trace: one `IncrVerifyEvent` per cache-hit query call when
/// `GLYIM_INCR_VERIFY=1`. Empty when the seam is disabled. Stored on
/// `Database` as a `RwLock` so concurrent readers do not block.
pub type IncrVerifyTrace = RwLock<Vec<IncrVerifyEvent>>;

/// `Database` M6.8 verify-trace accessors.
///
/// The typed `BodyQueries` trait in `glyim-pipeline` calls `record_incr_verify`
/// on every `Hit` / `Backdated` path when the seam is enabled; consumers
/// (`--explain-rebuild`, the nightly harness) read the snapshot.
impl crate::Database {
    /// Whether the M6.8 verify seam is enabled for this `Database`. Read once
    /// from `GLYIM_INCR_VERIFY` at construction (so the hot path is a single
    /// `AtomicBool::load`, not a `getenv` per query) and overridable per-
    /// `Database` via [`Database::set_incr_verify_for_test`].
    ///
    /// The typed `BodyQueries` accessors call this (not the free function) so
    /// the per-`Database` override is honoured: a test that flips this flag
    /// does not race with the parallel test runner's process-global env.
    #[must_use]
    pub fn incr_verify_enabled(&self) -> bool {
        self.incr_verify_enabled.load(Ordering::Relaxed)
    }

    /// **Test-only:** override the M6.8 verify-seam enable flag on this
    /// `Database`. Used by the `incr_verify_tests` to enable/disable the seam
    /// per-test without mutating the process-global `GLYIM_INCR_VERIFY` env
    /// var (edition 2024 made `set_var` unsafe and the parallel test runner
    /// makes env mutation flaky).
    ///
    /// Production code MUST NOT call this: the seam's enable state is a
    /// deployment concern, owned by the env var at `Database::new`. The
    /// `_for_test` suffix is the only guard; a future ratchet (plan M3.5
    /// panic burn-down) will move this behind a `test-support` feature.
    pub fn set_incr_verify_for_test(&self, enabled: bool) {
        self.incr_verify_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Append one M6.8 verify event to the trace. Called by the typed
    /// `BodyQueries` accessors on a cache hit when the seam is enabled. No-op
    /// (the caller skips the call) when the seam is disabled — the trace
    /// stays empty.
    pub fn record_incr_verify(&self, kind: QueryKind, id: u64, outcome: IncrVerifyOutcome) {
        self.incr_verify_trace
            .write()
            .push(IncrVerifyEvent { kind, id, outcome });
    }

    /// A sorted, deterministic snapshot of the M6.8 verify trace. The events
    /// are sorted by `(kind, id, outcome)` so two runs over the same compile
    /// produce byte-identical snapshots regardless of query issue order
    /// (RG-04). The trace is NOT cleared — `clear_incr_verify` resets it
    /// between compiles.
    #[must_use]
    pub fn incr_verify_snapshot(&self) -> Vec<IncrVerifyEvent> {
        let mut events = self.incr_verify_trace.read().clone();
        events.sort_unstable();
        events
    }

    /// The number of recorded `Mismatch` outcomes (the disagreement count
    /// the nightly harness fails CI on). `0` is the M6.8 `DoD`.
    #[must_use]
    pub fn incr_verify_mismatch_count(&self) -> usize {
        self.incr_verify_trace
            .read()
            .iter()
            .filter(|e| e.outcome == IncrVerifyOutcome::Mismatch)
            .count()
    }

    /// Clear the M6.8 verify trace. Called between compiles so each snapshot
    /// reflects one compile (mirrors `clear_rebuild_trace`).
    pub fn clear_incr_verify(&self) {
        self.incr_verify_trace.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CrateConfig, Database};

    /// `incr_verify_enabled_for` is `true` iff the value is exactly `Some("1")`.
    /// Tested via the pure helper so the parallel test runner does not race on
    /// process-global env mutation (edition 2024 made `set_var` unsafe).
    #[test]
    fn env_var_gate_is_one_only() {
        assert!(!incr_verify_enabled_for(None));
        assert!(incr_verify_enabled_for(Some("1")));
        assert!(!incr_verify_enabled_for(Some("0")));
        assert!(!incr_verify_enabled_for(Some("true")));
        assert!(!incr_verify_enabled_for(Some("yes")));
        assert!(!incr_verify_enabled_for(Some("")));
    }

    /// `record_incr_verify` appends; `incr_verify_snapshot` sorts by
    /// `(kind, id, outcome)` for determinism (derive order: `ThirBody` <
    /// `MirBuild` < `DefMap`).
    #[test]
    fn snapshot_is_sorted_and_deterministic() {
        let db = Database::new(CrateConfig {
            name: "t".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            opt_level: 0,
        });
        // Insert out-of-order: DefMap first, then high-id ThirBody, then
        // low-id ThirBody.
        db.record_incr_verify(QueryKind::DefMap, 5, IncrVerifyOutcome::Mismatch);
        db.record_incr_verify(QueryKind::ThirBody, 30, IncrVerifyOutcome::Verified);
        db.record_incr_verify(QueryKind::ThirBody, 5, IncrVerifyOutcome::Verified);

        let snap = db.incr_verify_snapshot();
        // Sorted by (kind, id, outcome): ThirBody < MirBuild < DefMap; within
        // ThirBody, id 5 < id 30.
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].kind, QueryKind::ThirBody);
        assert_eq!(snap[0].id, 5);
        assert_eq!(snap[1].kind, QueryKind::ThirBody);
        assert_eq!(snap[1].id, 30);
        assert_eq!(snap[2].kind, QueryKind::DefMap);
        assert_eq!(snap[2].id, 5);
    }

    /// `incr_verify_mismatch_count` counts only `Mismatch` outcomes.
    #[test]
    fn mismatch_count_filters_verified() {
        let db = Database::new(CrateConfig {
            name: "t".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            opt_level: 0,
        });
        db.record_incr_verify(QueryKind::ThirBody, 1, IncrVerifyOutcome::Verified);
        db.record_incr_verify(QueryKind::ThirBody, 2, IncrVerifyOutcome::Mismatch);
        db.record_incr_verify(QueryKind::MirBuild, 3, IncrVerifyOutcome::Verified);
        db.record_incr_verify(QueryKind::MirBuild, 4, IncrVerifyOutcome::Mismatch);
        assert_eq!(db.incr_verify_mismatch_count(), 2);

        db.clear_incr_verify();
        assert_eq!(db.incr_verify_mismatch_count(), 0);
    }
}
