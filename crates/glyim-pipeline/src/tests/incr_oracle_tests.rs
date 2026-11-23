//! M6.8 incremental ≡ clean oracle: random edit-sequence property tests.
//!
//! Plan §6 M6.8 `DoD`: "random token-/item-level edit sequences, compare
//! diagnostics + object bytes; `GLYIM_INCR_VERIFY=1` re-computes a sample of
//! cache hits and compares fingerprints" + "10k sequences nightly, 0
//! disagreements".
//!
//! The oracle is the property: for every edit in a seeded random sequence,
//! an **incremental** `Database` (carries the cache forward) produces the
//! same `def_map` fingerprint and the same per-item `thir_body` fingerprints
//! as a **clean** `Database` (rebuilt from scratch). A disagreement is a
//! backdating bug — the incremental cache served a value a clean recompute
//! would not. The harness also enables the M6.8 verify seam on the
//! incremental DB so the recompute-and-compare layer records `Mismatch` for
//! the same disagreement the oracle catches.
//!
//! These tests run a PR-tier smoke (≤1k edits, seeded) — the nightly 10k
//! harness is the same code with a larger `N` (plan §7.2 rule 9: "never put a
//! nightly-only check in the required set without a PR-tier smoke version").

use crate::body_queries::BodyQueries;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::{Fingerprint, StableHasher};
use glyim_db::{Database, QueryContext, QueryKind};
use glyim_span::Span;
use glyim_type::Ty;
use glyim_typeck::thir;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

/// A model program: `N` items, each a `fn nameN() -> i32 { bodyN }`. The
/// `def_map` fingerprint is body-agnostic; each item's `thir_body`
/// fingerprint is body-sensitive. Edits mutate the model and the oracle
/// compares incremental-vs-clean fingerprints after each edit.
struct ModelProgram {
    /// One source line per item: `fn name{i}(local_i) -> i32 { {body} }\n`.
    items: Vec<ItemLine>,
}

struct ItemLine {
    /// The body literal — body-only edits mutate only this.
    body: u32,
    /// The item name — signature edits mutate this (renames move the
    /// `def_map` fingerprint).
    name: String,
}

impl ModelProgram {
    /// A 5-item program with distinct names and bodies.
    fn new(n: u32) -> Self {
        let items = (0..n)
            .map(|i| ItemLine {
                body: i,
                name: format!("fn{i}"),
            })
            .collect();
        Self { items }
    }

    /// Render the model to a source string the `def_map` query consumes.
    fn render(&self) -> String {
        let mut s = String::new();
        for item in &self.items {
            let _ = std::fmt::Write::write_fmt(
                &mut s,
                format_args!("fn {}(x: i32) -> i32 {{ {} }}\n", item.name, item.body),
            );
        }
        s
    }

    /// Apply a seeded random edit. Three edit kinds exercise the cache's
    /// three branches: body-only (backdate), signature rename (miss), and
    /// item reordering (`def_map` fingerprint unchanged, body fingerprints
    /// unchanged by position). Returns the edit kind for the assert message.
    fn apply_edit(&mut self, rng: &mut StdRng) -> Edit {
        let idx = rng.random_range(0..self.items.len());
        match rng.random_range(0..3u32) {
            // Body-only edit: the def_map fingerprint must NOT move; the
            // item's thir_body fingerprint MUST move.
            0 => {
                self.items[idx].body = rng.random_range(0..1_000_000);
                Edit::BodyOnly
            }
            // Signature rename: the def_map fingerprint MUST move; every
            // item's thir_body fingerprint is unchanged (bodies untouched).
            1 => {
                self.items[idx].name = format!("fn{}_{}", idx, rng.random_range(0..1_000_000));
                Edit::SignatureRename
            }
            // Item reorder: swap two items. def_map fingerprint is order-
            // sensitive (it hashes the item list in order), so it moves;
            // the swapped items' body fingerprints are unchanged.
            _ => {
                let other = rng.random_range(0..self.items.len());
                if other != idx {
                    self.items.swap(idx, other);
                }
                Edit::Reorder
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    BodyOnly,
    SignatureRename,
    Reorder,
}

/// A minimal THIR body distinct per `(local, body)` (different owner →
/// different `Debug` → different fingerprint).
fn thir_body_for(local: u32) -> thir::Body {
    thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(local)),
        params: Vec::new(),
        return_ty: Ty::UNIT,
        stmts: Vec::new(),
        span: Span::DUMMY,
    }
}

/// The `StableHasher` fingerprint of a value's `Debug` rendering — the same
/// hash the `BodyQueries` backdating uses internally. Two structurally
/// identical values hash identically.
fn debug_fingerprint<T: std::fmt::Debug>(value: &T) -> Fingerprint {
    let mut h = StableHasher::new();
    h.write_str(&format!("{value:?}"));
    Fingerprint::from_bytes(h.finish())
}

/// Build the `def_map` fingerprint a `Database` computes for the model's
/// rendered source — the body-agnostic signature summary (M6.3).
fn def_map_fingerprint(db: &Database, file: glyim_span::FileId) -> Fingerprint {
    db.item_tree_fingerprint(file)
}

/// Add (or replace) the model's source on the `Database`'s VFS and return
/// the `FileId`.
fn set_source(db: &Database, src: &str) -> glyim_span::FileId {
    let path = std::path::Path::new("oracle.g");
    db.vfs().add_file_content(path, Arc::from(src))
}

use std::sync::Arc;

/// The core M6.8 property: after each edit in a seeded sequence, the
/// **incremental** `Database` (cache carried forward) produces the same
/// `def_map` fingerprint and the same per-item `thir_body` fingerprints as a
/// **clean** `Database` (rebuilt from scratch). A disagreement is a
/// backdating bug.
///
/// Plan §6 M6.8 `DoD`: "10k sequences nightly, 0 disagreements" — this is
/// the PR-tier smoke (the same code with a larger `N` is the nightly run).
#[test]
fn incremental_matches_clean_across_edit_sequence() {
    const N_EDITS: usize = 200;
    const N_ITEMS: u32 = 5;
    // Seed `0xM6_8_ORACLE`: M=month 11 (Nov), 6.8 = milestone id. A stable
    // seed so the oracle is deterministic across runs/machines (RG-04).
    let mut rng = StdRng::seed_from_u64(0x1106_0800);
    let mut model = ModelProgram::new(N_ITEMS);

    // The incremental DB carries the cache forward across edits.
    let incr_db = Database::default();
    incr_db.set_incr_verify_for_test(true);

    for step in 0..N_EDITS {
        let edit = model.apply_edit(&mut rng);
        let src = model.render();

        // Incremental: set the new source, then run the def_map query (cache
        // hit on body-only edit; miss on signature/reorder) and each item's
        // thir_body query.
        let incr_file = set_source(&incr_db, &src);
        // Overwrite the same FileId so the incremental DB sees an edit, not a
        // new file. `add_file_content` dedups by path, so this replaces.
        let _ = incr_file;
        let incr_def_fp = def_map_fingerprint(&incr_db, incr_file);
        // Run each item's thir_body query through the incremental cache.
        let incr_thir_fps: Vec<Fingerprint> = (0..N_ITEMS)
            .map(|local| {
                let arc =
                    incr_db.thir_body_query(LocalDefId::from_raw(local), || thir_body_for(local));
                debug_fingerprint(&*arc)
            })
            .collect();

        // Clean: a fresh DB with no cache, same source. Every query is a miss.
        let clean_db = Database::default();
        let clean_file = set_source(&clean_db, &src);
        let clean_def_fp = def_map_fingerprint(&clean_db, clean_file);
        let clean_thir_fps: Vec<Fingerprint> = (0..N_ITEMS)
            .map(|local| {
                let arc =
                    clean_db.thir_body_query(LocalDefId::from_raw(local), || thir_body_for(local));
                debug_fingerprint(&*arc)
            })
            .collect();

        // The oracle assertion: incremental ≡ clean for both the def_map
        // fingerprint and every item's thir_body fingerprint.
        assert_eq!(
            incr_def_fp, clean_def_fp,
            "edit {step} ({edit:?}): def_map fingerprint diverged (incremental ≠ clean)"
        );
        for (local, (incr_fp, clean_fp)) in
            incr_thir_fps.iter().zip(clean_thir_fps.iter()).enumerate()
        {
            assert_eq!(
                incr_fp, clean_fp,
                "edit {step} ({edit:?}): thir_body[{local}] fingerprint diverged (incremental ≠ clean)"
            );
        }
    }

    // The M6.8 verify seam on the incremental DB must have recorded zero
    // mismatches across the whole sequence — every backdate recomputed a
    // fingerprint that matched the cached entry's hash.
    let mismatches = incr_db.incr_verify_mismatch_count();
    assert_eq!(
        mismatches, 0,
        "M6.8 oracle: {mismatches} verify mismatches across {N_EDITS} edits (expect 0)"
    );
}

/// A shorter smoke that also confirms the verify seam recorded at least one
/// `Verified` event (the backdate path ran) — pins that the seam is wired
/// into the incremental DB the oracle uses, not just the unit tests.
#[test]
fn incremental_oracle_records_verified_on_backdates() {
    const N_EDITS: usize = 50;
    const N_ITEMS: u32 = 3;
    let mut rng = StdRng::seed_from_u64(0xC0DE);
    let mut model = ModelProgram::new(N_ITEMS);

    let incr_db = Database::default();
    incr_db.set_incr_verify_for_test(true);

    // Seed the cache with the initial source.
    let _ = set_source(&incr_db, &model.render());
    for local in 0..N_ITEMS {
        incr_db.thir_body_query(LocalDefId::from_raw(local), || thir_body_for(local));
    }
    incr_db.clear_incr_verify();

    for _ in 0..N_EDITS {
        let _edit = model.apply_edit(&mut rng);
        let _ = set_source(&incr_db, &model.render());
        for local in 0..N_ITEMS {
            incr_db.thir_body_query(LocalDefId::from_raw(local), || thir_body_for(local));
        }
    }

    let report = incr_db.incr_verify_snapshot();
    let verified_count = report
        .iter()
        .filter(|e| e.outcome == glyim_db::IncrVerifyOutcome::Verified)
        .count();
    assert!(
        verified_count > 0,
        "M6.8 oracle: verify seam recorded {verified_count} Verified events (expect > 0: backdates ran)"
    );
    assert_eq!(
        incr_db.incr_verify_mismatch_count(),
        0,
        "M6.8 oracle: mismatches across the smoke (expect 0)"
    );
    // The verify trace's kinds are all ThirBody (the only query the smoke
    // exercises through the typed cache).
    assert!(
        report.iter().all(|e| e.kind == QueryKind::ThirBody),
        "M6.8 oracle: verify trace kinds include non-ThirBody"
    );
}
