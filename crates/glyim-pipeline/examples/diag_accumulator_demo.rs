//! M6.5 diagnostic-accumulator consumer example.
//!
//! Demonstrates [`BodyQueries::mir_build_query_with_diags`] on [`Database`]:
//! a `mir_build` query whose `compute` closure emits non-fatal diagnostics
//! caches those diagnostics with the body and **replays them verbatim** on a
//! cache hit (the plan §6 M6.5 definition of done: "cached run prints
//! identical diagnostics"). The second call passes a `compute` closure that
//! panics — proving the cache hit never recomputes — and returns the same
//! `Arc<Body>` *and* the same `Arc<Vec<GlyimDiagnostic>>` the first run did.
//!
//! # Run
//!
//! ```sh
//! cargo run -p glyim-pipeline --no-default-features --example diag_accumulator_demo
//! ```
//!
//! Expected output: the initial query caches the body with two warnings
//! (stable-sorted: `a` before `b`); the second query cache-hits and replays
//! the *same* diagnostic allocation (`Arc::ptr_eq` = true, `compute` not
//! called).

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{CrateConfig, Database};
use glyim_diag::GlyimDiagnostic;
use glyim_mir::Body;
use glyim_pipeline::body_queries::BodyQueries;
use glyim_span::Span;
use std::sync::Arc;

/// A valid (M5.5-validator-passing) dummy MIR body: one `Unreachable`
/// terminator block + a single `UNIT` return place, no args/stmts/drops.
fn dummy_body(owner: DefId) -> Body {
    Body::dummy(owner)
}

fn main() {
    let db = Database::new(CrateConfig {
        name: "demo".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    // A frozen TyCtx the M5.5 validator reads; `Body::dummy` has no `Drop`
    // terminator so the drop-glue check is skipped and the ctx content is
    // irrelevant for this body.
    let ty_ctx = glyim_test::test_ty_ctx().freeze();
    let owner = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(1));

    // First call: cache miss → compute runs, emits two warnings out of stable
    // order, the body is validated (M5.5) and cached WITH the (stable-sorted)
    // diagnostics.
    let (arc1, diags1) = db
        .mir_build_query_with_diags(owner, &ty_ctx, || {
            Ok((
                dummy_body(owner),
                vec![
                    GlyimDiagnostic::warning(Span::DUMMY, "unused local b"),
                    GlyimDiagnostic::warning(Span::DUMMY, "unused local a"),
                ],
            ))
        })
        .expect("first compute succeeds and validates");
    println!(
        "initial mir_build cached ({} diagnostics, stable-sorted)",
        diags1.len()
    );
    for d in diags1.iter() {
        println!("  - {:?}: {}", d.severity, d.message);
    }

    // Second call: cache hit → compute must NOT run (the closure panics) and
    // the replayed diagnostics are the SAME allocation the first run cached.
    let (arc2, diags2) = db
        .mir_build_query_with_diags(owner, &ty_ctx, || panic!("cache hit must not recompute"))
        .expect("cache hit returns Ok");
    println!(
        "after cache hit: same body Arc = {}, same diagnostics Arc = {} (compute not called)",
        Arc::ptr_eq(&arc1, &arc2),
        Arc::ptr_eq(&diags1, &diags2),
    );
}
