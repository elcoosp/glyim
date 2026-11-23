//! M6.8 incremental ≡ clean oracle: consumer example.
//!
//! Demonstrates the M6.8 incremental ≡ clean oracle (plan §6 M6.8 `DoD`:
//! "random token-/item-level edit sequences, compare diagnostics + object
//! bytes"). For each edit in a seeded sequence, an **incremental**
//! `Database` (cache carried forward) is compared against a **clean**
//! `Database` (rebuilt from scratch): the `def_map` fingerprint and every
//! item's `thir_body` fingerprint must agree. A disagreement is a
//! backdating bug — the incremental cache served a value a clean recompute
//! would not.
//!
//! # Run
//!
//! ```sh
//! cargo run -p glyim-pipeline --no-default-features --example incr_oracle_demo
//! ```
//!
//! Expected output: the oracle runs `N_EDITS` edits, prints per-step
//! disagreement counts, and the final pass/fail signal (`mismatch_count` is
//! the number the nightly 10k-sequence harness asserts is `0`).

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::{Fingerprint, StableHasher};
use glyim_db::{Database, QueryContext};
use glyim_pipeline::body_queries::BodyQueries;
use glyim_span::Span;
use glyim_type::Ty;
use glyim_typeck::thir;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use std::sync::Arc;

const N_EDITS: usize = 30;
const N_ITEMS: u32 = 4;

/// A minimal THIR body distinct per `local`.
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
/// hash the `BodyQueries` backdating uses internally.
fn debug_fingerprint<T: std::fmt::Debug>(value: &T) -> Fingerprint {
    let mut h = StableHasher::new();
    h.write_str(&format!("{value:?}"));
    Fingerprint::from_bytes(h.finish())
}

struct Model {
    bodies: Vec<u32>,
    names: Vec<String>,
}

impl Model {
    fn new() -> Self {
        let bodies = (0..N_ITEMS).collect();
        let names = (0..N_ITEMS).map(|i| format!("fn{i}")).collect();
        Self { bodies, names }
    }

    fn render(&self) -> String {
        let mut s = String::new();
        for i in 0..N_ITEMS as usize {
            let _ = std::fmt::Write::write_fmt(
                &mut s,
                format_args!(
                    "fn {}(x: i32) -> i32 {{ {} }}\n",
                    self.names[i], self.bodies[i]
                ),
            );
        }
        s
    }

    fn apply_edit(&mut self, rng: &mut StdRng) -> &'static str {
        let idx = rng.random_range(0..N_ITEMS as usize);
        match rng.random_range(0..3u32) {
            0 => {
                self.bodies[idx] = rng.random_range(0..1_000_000);
                "body-only"
            }
            1 => {
                self.names[idx] = format!("fn{idx}_{}", rng.random_range(0..1_000_000));
                "signature-rename"
            }
            _ => {
                let other = rng.random_range(0..N_ITEMS as usize);
                if other != idx {
                    self.bodies.swap(idx, other);
                    self.names.swap(idx, other);
                }
                "reorder"
            }
        }
    }
}

fn set_source(db: &Database, src: &str) -> glyim_span::FileId {
    db.vfs()
        .add_file_content(std::path::Path::new("oracle.g"), Arc::from(src))
}

fn main() {
    let mut rng = StdRng::seed_from_u64(0x1106_0800);
    let mut model = Model::new();

    // The incremental DB carries the cache forward across edits; the verify
    // seam is enabled so a disagreement is also recorded as a `Mismatch`.
    let incr_db = Database::default();
    incr_db.set_incr_verify_for_test(true);

    let mut total_disagreements = 0usize;
    for step in 0..N_EDITS {
        let edit = model.apply_edit(&mut rng);
        let src = model.render();

        // Incremental: set source, run def_map + thir_body queries.
        let incr_file = set_source(&incr_db, &src);
        let incr_def_fp = incr_db.item_tree_fingerprint(incr_file);
        let incr_thir_fps: Vec<Fingerprint> = (0..N_ITEMS)
            .map(|local| {
                let arc =
                    incr_db.thir_body_query(LocalDefId::from_raw(local), || thir_body_for(local));
                debug_fingerprint(&*arc)
            })
            .collect();

        // Clean: fresh DB, same source.
        let clean_db = Database::default();
        let clean_file = set_source(&clean_db, &src);
        let clean_def_fp = clean_db.item_tree_fingerprint(clean_file);
        let clean_thir_fps: Vec<Fingerprint> = (0..N_ITEMS)
            .map(|local| {
                let arc =
                    clean_db.thir_body_query(LocalDefId::from_raw(local), || thir_body_for(local));
                debug_fingerprint(&*arc)
            })
            .collect();

        let def_agree = incr_def_fp == clean_def_fp;
        let thir_agree = incr_thir_fps == clean_thir_fps;
        let step_disagree = usize::from(!(def_agree && thir_agree));
        total_disagreements += step_disagree;
        println!("edit {step:2} ({edit:18}): def_map agree={def_agree}, thir agree={thir_agree}");
    }

    println!();
    let mismatches = incr_db.incr_verify_mismatch_count();
    println!("== M6.8 oracle result ==");
    println!("edits:                {N_EDITS}");
    println!("disagreements:         {total_disagreements} (expect 0: incremental ≡ clean)");
    println!("verify mismatches:     {mismatches} (expect 0: backdating correct)");
    println!();
    println!(
        "pass/fail: {} the nightly 10k-sequence gate (plan M6.8 `DoD`: \"0 disagreements\")",
        if total_disagreements == 0 && mismatches == 0 {
            "PASSES"
        } else {
            "FAILS"
        }
    );
}
