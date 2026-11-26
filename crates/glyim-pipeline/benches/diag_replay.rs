//! M6.5 diagnostic-accumulator replay-overhead bench.
//!
//! The accumulator's cost must be invisible against any real compile: a
//! cache hit returns the cached `Arc<Body>` *and* replays the cached
//! `Arc<Vec<GlyimDiagnostic>>` (one `Arc::clone` + one `HashMap` read), and
//! a cache miss pays one stable sort of the emitted diagnostics. This bench
//! prints the per-call cost of both paths so CI logs can track headroom
//! against the fresh-compute baseline. Run with:
//!
//! ```text
//! cargo bench -p glyim-pipeline --no-default-features --bench diag_replay
//! ```
//!
//! (Dependency-free `harness = false`, mirroring the `phase_prof` bench: the
//! accumulator runs once per `mir_build` query, so its cost is dwarfed by
//! typeck/lower — this bench keeps the measurement honest without pulling in
//! criterion/divan.)

use std::hint::black_box;
use std::time::Instant;

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{CrateConfig, Database, QueryKind};
use glyim_diag::GlyimDiagnostic;
use glyim_mir::Body;
use glyim_pipeline::body_queries::BodyQueries;
use glyim_span::Span;

/// Number of iterations per measured path. Large enough that the per-op cost
/// is well above timer resolution, small enough that the bench finishes in
/// well under a second.
const ITERS: usize = 20_000;

/// The number of diagnostics each `compute` emits — exercises the stable
/// sort on the miss path and the `Arc` clone on the hit path.
const DIAGS_PER_BODY: usize = 8;

/// Builds the database + owner + frozen `TyCtx` the bench reuses across paths.
fn setup() -> (Database, DefId, glyim_type::TyCtx) {
    let db = Database::new(CrateConfig {
        name: "bench".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let owner = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(1));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();
    (db, owner, ty_ctx)
}

/// The diagnostics a `compute` closure emits — `DIAGS_PER_BODY` warnings in
/// reverse message order so the accumulator's stable sort does real work.
fn emitted_diags() -> Vec<GlyimDiagnostic> {
    (0..DIAGS_PER_BODY)
        .map(|i| GlyimDiagnostic::warning(Span::DUMMY, format!("unused local {i}")))
        .rev()
        .collect()
}

/// A valid (M5.5-validator-passing) dummy MIR body.
fn dummy_body(owner: DefId) -> Body {
    Body::dummy(owner)
}

fn main() {
    let (db, owner, ty_ctx) = setup();
    let id = {
        let krate = owner.krate;
        let local = owner.local_id;
        (u64::from(krate.to_raw()) << 32) | u64::from(local.to_raw())
    };

    // Warm the cache once so the hit path measures a real cache hit, not the
    // first-ever insert.
    let _ =
        db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((dummy_body(owner), emitted_diags())));

    // --- Cache-hit replay path (the M6.5 hot path) ---
    let start = Instant::now();
    for _ in 0..ITERS {
        let (arc, diags) = db
            .mir_build_query_with_diags(owner, &ty_ctx, || panic!("hit must not recompute"))
            .expect("hit");
        black_box(arc);
        black_box(diags);
    }
    let hit_elapsed = start.elapsed();
    assert!(
        db.query_cache_diagnostics(QueryKind::MirBuild, id)
            .is_some(),
        "hit path must not have invalidated the cache"
    );

    // --- Fresh-compute (cache-miss) path: invalidate before each call so
    // `compute` + validate + stable-sort + insert run every iteration. ---
    let start = Instant::now();
    for _ in 0..ITERS {
        db.query_cache_invalidate(QueryKind::MirBuild, id);
        let (arc, diags) = db
            .mir_build_query_with_diags(owner, &ty_ctx, || Ok((dummy_body(owner), emitted_diags())))
            .expect("miss");
        black_box(arc);
        black_box(diags);
    }
    let miss_elapsed = start.elapsed();

    let hit_ns = hit_elapsed.as_nanos() as f64 / ITERS as f64;
    let miss_ns = miss_elapsed.as_nanos() as f64 / ITERS as f64;
    println!(
        "M6.5 diag accumulator: {DIAGS_PER_BODY} diags/body, {ITERS} iters\n\
         \x20  cache-hit replay : {hit_ns:8.1} ns/op\n\
         \x20  fresh compute     : {miss_ns:8.1} ns/op\n\
         \x20  replay overhead   : {:8.1} ns/op ({:.2}% of fresh)",
        hit_ns,
        100.0 * hit_ns / miss_ns,
    );
}
