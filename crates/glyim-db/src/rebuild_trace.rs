//! M6.9 query-rebuild trace: records which body/signature queries recomputed
//! vs cache-hit/backdated during a compile, so `--explain-rebuild` can surface
//! the minimal recompute set (plan §6 M6.9, `DoD`: "documented output format").
//!
//! The trace is the consumer view of the M6.4/M6.5 query cache: every
//! [`BodyQueries`](../../glyim_pipeline/body_queries/trait.BodyQueries.html)
//! method records one `RebuildOutcome` per call. A fresh compile records all
//! `Miss`es; an incremental re-compile records `Hit`/`Backdated` for the
//! queries that did not recompute and `Miss` for the ones that did — exactly
//! the set `--explain-rebuild` prints.
//!
//! # Determinism
//!
//! The snapshot is sorted by `(QueryKind, id)` before rendering, so two runs
//! over the same compile produce byte-identical output regardless of the
//! order queries were issued. No clock, PRNG, or `HashMap` iteration reaches
//! the rendered text (RG-04 / RG-06).

use parking_lot::RwLock;
use std::fmt::Write as _;

use crate::body_queries::QueryKind;

/// The outcome a single body/signature query recorded for one call.
///
/// The three values are the exhaustive set of things a query can do with its
/// cache entry, and they are what `--explain-rebuild` prints per query.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RebuildOutcome {
    /// The cache entry was present and the query returned it without running
    /// `compute` (`mir_build` after a backdated `thir_body`).
    Hit,
    /// `compute` ran, the recomputed content hash matched the cached entry's
    /// hash, and the cached value was returned unchanged (early cutoff /
    /// backdating — `thir_body` and `def_map`).
    Backdated,
    /// `compute` ran and produced fresh content (cache empty or hash moved);
    /// the new value was cached and downstream entries invalidated.
    Miss,
}

impl RebuildOutcome {
    /// The lowercase token used in the `--explain-rebuild` table column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RebuildOutcome::Hit => "hit",
            RebuildOutcome::Backdated => "backdated",
            RebuildOutcome::Miss => "miss",
        }
    }
}

/// A single recorded query-rebuild event: which query family, which id, and
/// what its cache did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RebuildEvent {
    /// The query family (`thir_body` / `mir_build` / `def_map`).
    pub kind: QueryKind,
    /// The cache id the query was keyed on (local def id for `thir_body`,
    /// `(krate << 32) | local` for `mir_build`, file id for `def_map`).
    pub id: u64,
    /// What the cache did for this call.
    pub outcome: RebuildOutcome,
}

/// The M6.9 query-rebuild trace: an append-only `Vec<RebuildEvent>` behind a
/// `RwLock` so the read path (`--explain-rebuild` rendering, tests) does not
/// block the query methods that record into it.
pub type QueryRebuildTrace = RwLock<Vec<RebuildEvent>>;

/// `Database` M6.9 query-rebuild-trace accessors.
///
/// The typed [`BodyQueries`](../../glyim_pipeline/body_queries/trait.BodyQueries.html)
/// methods in `glyim-pipeline` call [`Database::record_rebuild`] at each
/// cache decision; the CLI's `--explain-rebuild` reads
/// [`Database::render_rebuild_report`] after a compile.
impl crate::Database {
    /// Record one query-rebuild event. Called by the typed `BodyQueries`
    /// methods at each cache hit/backdate/miss decision.
    pub fn record_rebuild(&self, kind: QueryKind, id: u64, outcome: RebuildOutcome) {
        self.rebuild_trace
            .write()
            .push(RebuildEvent { kind, id, outcome });
    }

    /// A deterministic snapshot of the trace: all events, sorted by
    /// `(kind label, id, outcome)`. The kind label (`def_map` < `mir_build` <
    /// `thir_body`) — not the enum discriminant — is the sort key so the
    /// rendered table reads in the natural alphabetical order a human
    /// expects, and two runs over the same compile return byte-identical
    /// snapshots regardless of query issue order.
    #[must_use]
    pub fn rebuild_trace_snapshot(&self) -> Vec<RebuildEvent> {
        let mut events = self.rebuild_trace.read().clone();
        events.sort_by_key(|e| (query_kind_label(e.kind), e.id, e.outcome));
        events
    }

    /// Clear the trace. The CLI calls this once per invocation before a
    /// compile so the report reflects exactly that compile (not any prior
    /// state on a reused `Database`).
    pub fn clear_rebuild_trace(&self) {
        self.rebuild_trace.write().clear();
    }

    /// The number of recorded events (test/diagnostic helper).
    #[must_use]
    pub fn rebuild_trace_len(&self) -> usize {
        self.rebuild_trace.read().len()
    }

    /// Render the rebuild report (M6.9 `DoD`: "documented output format") as
    /// the stable, deterministic text `--explain-rebuild` prints to stderr.
    ///
    /// # Format
    ///
    /// One header line naming the query count, then one fixed-width row per
    /// event sorted by `(kind, id)`:
    ///
    /// ```text
    /// --explain-rebuild: 3 queries recorded
    ///   kind        id                  outcome
    ///   def_map     0x0000000000000003  backdated
    ///   mir_build   0x0000000000000007  hit
    ///   thir_body   0x0000000000000007  miss
    /// ```
    ///
    /// `kind` is left-aligned in 11 columns; `id` is a zero-padded 16-digit
    /// hex (`0x`-prefixed) so the column width is stable across query
    /// families (`mir_build` ids pack `(krate << 32) | local` and are wider
    /// than `thir_body`/`def_map` ids); `outcome` is the lowercase token from
    /// [`RebuildOutcome::as_str`]. An empty trace renders the single line
    /// `--explain-rebuild: 0 queries recorded (cache hit — nothing recomputed)`.
    #[must_use]
    pub fn render_rebuild_report(&self) -> String {
        let events = self.rebuild_trace_snapshot();
        let mut out = String::new();
        if events.is_empty() {
            out.push_str(
                "--explain-rebuild: 0 queries recorded (cache hit — nothing recomputed)\n",
            );
            return out;
        }
        let count = events.len();
        writeln!(
            out,
            "--explain-rebuild: {count} queries recorded\n  kind        id                  outcome"
        )
        .expect("writing to a String never fails");
        for ev in events {
            writeln!(
                out,
                "  {:<11} 0x{:016x} {}",
                query_kind_label(ev.kind),
                ev.id,
                ev.outcome.as_str()
            )
            .expect("writing to a String never fails");
        }
        out
    }
}

/// The lowercase label `--explain-rebuild` prints for a [`QueryKind`] (the
/// query family name a human reads, not the `Debug` rendering).
#[must_use]
fn query_kind_label(kind: QueryKind) -> &'static str {
    match kind {
        QueryKind::ThirBody => "thir_body",
        QueryKind::MirBuild => "mir_build",
        QueryKind::DefMap => "def_map",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CrateConfig, Database};

    fn fresh_db() -> Database {
        Database::new(CrateConfig {
            name: "t".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            opt_level: 0,
        })
    }

    /// The snapshot is sorted by `(kind, id)` — deterministic regardless of
    /// issue order. This is the M6.9 `DoD`'s "documented output format"
    /// precondition: two runs over the same compile produce byte-identical
    /// text.
    #[test]
    fn snapshot_is_sorted_and_deterministic() {
        let db = fresh_db();
        // Record out of (kind, id) order.
        db.record_rebuild(QueryKind::MirBuild, 7, RebuildOutcome::Hit);
        db.record_rebuild(QueryKind::ThirBody, 7, RebuildOutcome::Miss);
        db.record_rebuild(QueryKind::DefMap, 3, RebuildOutcome::Backdated);
        let snap = db.rebuild_trace_snapshot();
        assert_eq!(
            snap,
            vec![
                RebuildEvent {
                    kind: QueryKind::DefMap,
                    id: 3,
                    outcome: RebuildOutcome::Backdated
                },
                RebuildEvent {
                    kind: QueryKind::MirBuild,
                    id: 7,
                    outcome: RebuildOutcome::Hit
                },
                RebuildEvent {
                    kind: QueryKind::ThirBody,
                    id: 7,
                    outcome: RebuildOutcome::Miss
                },
            ]
        );
    }

    /// The rendered report is the documented format: header with count, one
    /// fixed-width row per event, ids as zero-padded 16-digit hex.
    #[test]
    fn render_rebuild_report_is_the_documented_format() {
        let db = fresh_db();
        db.record_rebuild(QueryKind::DefMap, 3, RebuildOutcome::Backdated);
        db.record_rebuild(QueryKind::MirBuild, 7, RebuildOutcome::Hit);
        db.record_rebuild(QueryKind::ThirBody, 7, RebuildOutcome::Miss);
        let report = db.render_rebuild_report();
        assert_eq!(
            report,
            "--explain-rebuild: 3 queries recorded\n\
             \x20 kind        id                  outcome\n\
             \x20 def_map     0x0000000000000003 backdated\n\
             \x20 mir_build   0x0000000000000007 hit\n\
             \x20 thir_body   0x0000000000000007 miss\n"
        );
    }

    /// An empty trace renders the cache-hit note — the format a compile-cache
    /// hit (object already exists) or a parse-only mode produces.
    #[test]
    fn empty_trace_renders_cache_hit_note() {
        let db = fresh_db();
        assert_eq!(
            db.render_rebuild_report(),
            "--explain-rebuild: 0 queries recorded (cache hit — nothing recomputed)\n"
        );
    }

    /// `clear_rebuild_trace` empties the trace so a reused `Database` reports
    /// only the most recent compile.
    #[test]
    fn clear_empties_the_trace() {
        let db = fresh_db();
        db.record_rebuild(QueryKind::ThirBody, 1, RebuildOutcome::Miss);
        assert_eq!(db.rebuild_trace_len(), 1);
        db.clear_rebuild_trace();
        assert_eq!(db.rebuild_trace_len(), 0);
    }
}
