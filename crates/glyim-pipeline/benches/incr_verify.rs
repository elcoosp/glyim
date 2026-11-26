//! M6.8 incremental ≡ clean oracle: `GLYIM_INCR_VERIFY` recompute-overhead
//! bench.
//!
//! The verify seam's cost must be invisible against any real compile when
//! disabled, and a measurable-but-tolerable recompute when enabled: a disabled
//! seam is a single `AtomicBool::load` (one relaxed load, no branch), and an
//! enabled seam re-runs `compute` + re-hashes on every cache hit. This bench
//! prints the per-call cost of both paths so CI logs can track the
//! enabled-vs-disabled delta — the plan (§6 M6.8) ships the seam opt-in
//! precisely so the hot path stays free when the nightly harness is not
//! running. Run with:
//!
//! ```text
//! cargo bench -p glyim-pipeline --no-default-features --bench incr_verify
//! ```
//!
//! (Dependency-free `harness = false`, mirroring the `diag_replay` bench:
//! the verify re-run is dwarfed by typeck/lower, so this bench keeps the
//! measurement honest without pulling in criterion/divan.)

use std::hint::black_box;
use std::time::Instant;

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{CrateConfig, Database};
use glyim_mir::Body;
use glyim_pipeline::body_queries::BodyQueries;
use glyim_type::TyCtx;

/// Number of iterations per measured path. Large enough that the per-op cost
/// is well above timer resolution, small enough that the bench finishes in
/// well under a second.
const ITERS: usize = 20_000;

/// Builds the database + owner + frozen `TyCtx` the bench reuses across
/// paths, with the verify seam in the requested enable state.
fn setup(verify_enabled: bool) -> (Database, DefId, TyCtx) {
    let db = Database::new(CrateConfig {
        name: "bench".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    db.set_incr_verify_for_test(verify_enabled);
    let owner = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(1));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();
    (db, owner, ty_ctx)
}

/// A valid (M5.5-validator-passing) dummy MIR body.
fn dummy_body(owner: DefId) -> Body {
    Body::dummy(owner)
}

/// Measures the per-call cost of `mir_build_query_with_diags` on the cache-hit
/// path with the verify seam DISABLED — the hot path in production (the env
/// var is unset). The seam is a single `AtomicBool::load` + no recompute.
fn bench_hit_disabled() {
    let (db, owner, ty_ctx) = setup(false);
    // Seed the cache (first call is a miss — not measured).
    db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((dummy_body(owner), Vec::new())))
        .expect("seed");

    db.clear_incr_verify();
    let start = Instant::now();
    for _ in 0..ITERS {
        db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((dummy_body(owner), Vec::new())))
            .expect("hit");
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / ITERS as u32;
    println!("mir_build hit (verify DISABLED): {ITERS} iters, {elapsed:?} total, {per_op:?}/op");
    let mismatches = db.incr_verify_mismatch_count();
    println!("  mismatches recorded: {mismatches} (expect 0: verify disabled)");
    black_box(()); // keep the loop from being optimized away
}

/// Measures the per-call cost of `mir_build_query_with_diags` on the cache-hit
/// path with the verify seam ENABLED — the nightly-harness path. The seam
/// re-runs `compute`, re-validates, and re-hashes the fresh body to compare
/// against the cached body's hash.
fn bench_hit_enabled() {
    let (db, owner, ty_ctx) = setup(true);
    // Seed the cache (first call is a miss — not measured).
    db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((dummy_body(owner), Vec::new())))
        .expect("seed");
    db.clear_incr_verify();

    let start = Instant::now();
    for _ in 0..ITERS {
        db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((dummy_body(owner), Vec::new())))
            .expect("hit");
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / ITERS as u32;
    println!("mir_build hit (verify ENABLED):  {ITERS} iters, {elapsed:?} total, {per_op:?}/op");
    let mismatches = db.incr_verify_mismatch_count();
    println!("  mismatches recorded: {mismatches} (expect 0: deterministic compute)");
    black_box(());
}

/// Measures the per-call cost of `thir_body_query` on the backdate path with
/// the verify seam ENABLED — records `Verified` per call (the backdate
/// recomputes the fingerprint; the seam records the conclusion).
fn bench_thir_backdate_enabled() {
    let (db, _, _) = setup(true);
    let def_id = LocalDefId::from_usize(2);
    // A minimal THIR body distinct per `local`.
    let body = || glyim_typeck::thir::Body {
        owner: DefId::new(CrateId::from_usize(0), def_id),
        params: Vec::new(),
        return_ty: glyim_type::Ty::UNIT,
        stmts: Vec::new(),
        span: glyim_span::Span::DUMMY,
    };
    // Seed the cache.
    db.thir_body_query(def_id, body);
    db.clear_incr_verify();

    let start = Instant::now();
    for _ in 0..ITERS {
        db.thir_body_query(def_id, body);
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / ITERS as u32;
    println!(
        "thir_body backdate (verify ENABLED): {ITERS} iters, {elapsed:?} total, {per_op:?}/op"
    );
    let mismatches = db.incr_verify_mismatch_count();
    println!("  mismatches recorded: {mismatches} (expect 0: deterministic compute)");
    black_box(());
}

fn main() {
    println!("== M6.8 GLYIM_INCR_VERIFY recompute-overhead bench ==");
    println!();
    bench_hit_disabled();
    bench_hit_enabled();
    bench_thir_backdate_enabled();
    println!();
    println!("The enabled-vs-disabled delta is the seam's cost; the disabled");
    println!("path is the production hot path (env var unset).");
}
