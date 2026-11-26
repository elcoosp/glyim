//! M6.8 incremental ≡ clean oracle: per-edit overhead bench.
//!
//! The oracle runs N seeded random edits, and for each edit compares an
//! **incremental** `Database` (cache carried forward) against a **clean**
//! `Database` (rebuilt from scratch). The per-edit cost is the incremental
//! recompute + the clean baseline recompute; the delta is the cache's win
//! (the cache-hit path skips recompute on a body-only edit). This bench
//! prints the per-edit cost so CI logs can track headroom against the
//! nightly 10k-sequence run. Run with:
//!
//! ```text
//! cargo bench -p glyim-pipeline --no-default-features --bench incr_oracle
//! ```
//!
//! (Dependency-free `harness = false`, mirroring the `diag_replay` /
//! `incr_verify` benches: the oracle's cost is dominated by the clean
//! recompute, not the comparison, so this bench keeps the measurement honest
//! without pulling in criterion/divan.)

use std::hint::black_box;
use std::time::Instant;

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::Database;
use glyim_pipeline::body_queries::BodyQueries;
use glyim_span::Span;
use glyim_type::Ty;
use glyim_typeck::thir;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use std::sync::Arc;

/// Number of edits the bench runs. Small enough to finish in well under a
/// second; the nightly harness runs 10k (plan M6.8 `DoD`).
const N_EDITS: usize = 500;
const N_ITEMS: u32 = 5;

/// A minimal THIR body distinct per `local`.
fn thir_body_for(local: u32) -> thir::Body {
    thir::Body {
        owner: DefId::new(
            CrateId::from_usize(0),
            LocalDefId::from_usize(local as usize),
        ),
        params: Vec::new(),
        return_ty: Ty::UNIT,
        stmts: Vec::new(),
        span: Span::DUMMY,
    }
}

/// A model program: `N_ITEMS` fns, each `fn nameI(x: i32) -> i32 { bodyI }`.
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
            // SAFETY: write! to a String never fails (the underlying
            // `String` Write impl is infallible); the `let _ =` discards the
            // `fmt::Result` the way the codebase's other renderers do.
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

    fn apply_edit(&mut self, rng: &mut StdRng) {
        let idx = rng.random_range(0..N_ITEMS as usize);
        match rng.random_range(0..3u32) {
            0 => self.bodies[idx] = rng.random_range(0..1_000_000),
            1 => self.names[idx] = format!("fn{idx}_{}", rng.random_range(0..1_000_000)),
            _ => {
                let other = rng.random_range(0..N_ITEMS as usize);
                if other != idx {
                    self.bodies.swap(idx, other);
                    self.names.swap(idx, other);
                }
            }
        }
    }
}

fn set_source(db: &Database, src: &str) -> glyim_span::FileId {
    db.vfs()
        .add_file_content(std::path::Path::new("oracle.g"), Arc::from(src))
        .expect("fixture sources are far below the VFS length gate")
}

fn main() {
    println!("== M6.8 incremental ≡ clean oracle: per-edit overhead bench ==");
    println!();
    println!("{N_EDITS} edits × {N_ITEMS} items; incremental DB vs clean DB per edit.");

    let mut rng = StdRng::seed_from_u64(0x1106_0800);
    let mut model = Model::new();

    let incr_db = Database::default();
    incr_db.set_incr_verify_for_test(true);

    let start = Instant::now();
    let mut disagreements = 0usize;
    for _ in 0..N_EDITS {
        model.apply_edit(&mut rng);
        let src = model.render();

        // Incremental: set source, run def_map + thir_body queries (cache
        // hits on body-only edits; misses on signature/reorder).
        let incr_file = set_source(&incr_db, &src);
        let _ = incr_file;
        let incr_def_fp = glyim_db::QueryContext::item_tree_fingerprint(&incr_db, incr_file);
        for local in 0..N_ITEMS {
            let _ = incr_db.thir_body_query(LocalDefId::from_usize(local as usize), || {
                thir_body_for(local)
            });
        }

        // Clean: fresh DB, same source. Every query is a miss.
        let clean_db = Database::default();
        let clean_file = set_source(&clean_db, &src);
        let clean_def_fp = glyim_db::QueryContext::item_tree_fingerprint(&clean_db, clean_file);
        for local in 0..N_ITEMS {
            let _ = clean_db.thir_body_query(LocalDefId::from_usize(local as usize), || {
                thir_body_for(local)
            });
        }

        if incr_def_fp != clean_def_fp {
            disagreements += 1;
        }
    }
    let elapsed = start.elapsed();
    let per_edit = elapsed / N_EDITS as u32;

    println!(
        "{N_EDITS} edits, {elapsed:?} total, {per_edit:?}/edit (incremental + clean recompute)"
    );
    println!(
        "disagreements (def_map fingerprint): {disagreements} (expect 0: incremental ≡ clean)"
    );
    println!(
        "M6.8 verify mismatches on incremental DB: {} (expect 0)",
        incr_db.incr_verify_mismatch_count()
    );
    black_box(());
}
