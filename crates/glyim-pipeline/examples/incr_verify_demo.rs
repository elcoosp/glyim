//! M6.8 incremental ≡ clean oracle: `GLYIM_INCR_VERIFY` consumer example.
//!
//! Demonstrates the M6.8 verify seam on [`Database`]: when the seam is
//! enabled (the `GLYIM_INCR_VERIFY=1` env var at `Database::new`, or the
//! per-`Database` test override), a cache-hit query re-runs `compute`,
//! recomputes the content `Fingerprint`, and compares it to the cached
//! entry's hash. A match records `Verified`; a mismatch records `Mismatch`.
//!
//! # Run
//!
//! ```sh
//! cargo run -p glyim-pipeline --no-default-features --example incr_verify_demo
//! ```
//!
//! Expected output: the initial `mir_build` query is a cache miss (verify
//! does not run — no cached entry); the re-run is a cache hit with the seam
//! enabled, recording one `Verified` event and zero `Mismatch`s (the
//! deterministic `compute` produces a byte-identical body).

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{CrateConfig, Database, IncrVerifyOutcome, QueryKind};
use glyim_mir::Body;
use glyim_pipeline::body_queries::BodyQueries;

fn main() {
    let db = Database::new(CrateConfig {
        name: "demo".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    // Enable the M6.8 verify seam per-Database (production reads the
    // `GLYIM_INCR_VERIFY` env var at `Database::new`; the override keeps the
    // demo deterministic without env mutation).
    db.set_incr_verify_for_test(true);

    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();

    // Fresh: cache miss → compute + validate + cache. Verify does not run on
    // a miss (there is no cached entry to compare against).
    db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((Body::dummy(owner), Vec::new())))
        .expect("compute succeeds");
    let report = db.incr_verify_snapshot();
    println!(
        "fresh compute: verify events = {} (expect 0: miss does not verify)",
        report.len()
    );

    // Re-run with a structurally identical body → cache hit. The verify seam
    // re-runs `compute`, recomputes the body's fingerprint, and confirms it
    // matches the cached entry's → `Verified`.
    db.clear_incr_verify();
    db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((Body::dummy(owner), Vec::new())))
        .expect("cache hit returns Ok");
    let report = db.incr_verify_snapshot();
    println!(
        "cache hit (verify enabled): {} event(s), outcome = {:?} (expect Verified)",
        report.len(),
        report.first().map(|e| e.outcome),
    );
    println!(
        "  kind = {:?} (expect MirBuild), mismatches = {} (expect 0: deterministic compute)",
        report.first().map(|e| e.kind),
        db.incr_verify_mismatch_count(),
    );

    // The pass/fail signal: `incr_verify_mismatch_count()` is the single
    // number the nightly 10k-sequence harness (plan M6.8 `DoD`: "0
    // disagreements") asserts is `0`.
    let mismatches = db.incr_verify_mismatch_count();
    println!();
    println!(
        "M6.8 oracle pass/fail: mismatch_count = {mismatches} ({} the nightly gate)",
        if mismatches == 0 { "passes" } else { "FAILS" },
    );

    // Demonstrate the mismatch path: a deliberately-diverging `compute`
    // (two locals vs the cached one-local body) records `Mismatch`.
    db.clear_incr_verify();
    db.mir_build_query_with_diags(owner, &ty_ctx, || {
        let mut body = Body::dummy(owner);
        body.locals.push(glyim_mir::LocalDecl {
            ty: glyim_type::Ty::UNIT,
            mutability: glyim_core::primitives::Mutability::Not,
            source_info: glyim_mir::SourceInfo::new(glyim_span::Span::DUMMY),
        });
        Ok((body, Vec::new()))
    })
    .expect("cache hit still returns Ok (seam is observational)");
    let report = db.incr_verify_snapshot();
    println!(
        "diverging compute: outcome = {:?} (expect Mismatch), mismatch_count = {} (expect 1)",
        report.first().map(|e| e.outcome),
        db.incr_verify_mismatch_count(),
    );

    // The verify trace is sorted by (kind, id, outcome) for deterministic
    // `--explain-rebuild` rendering (RG-04 / RG-06).
    let _ = QueryKind::MirBuild;
    let _ = IncrVerifyOutcome::Mismatch;
}
