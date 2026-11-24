//! M2.7 quick win — `optimize` without whole-`Body` clone.
//!
//! Plan line pinned (docs/roadmaps/glyim-master-plan.md §6 M2.7 item 3,
//! restating §4.4 PF-12 "passes mutate in place (no whole-`Body` clone)"):
//!
//! > Quick-win campaign (each gated by measured instruction-count win, no
//! > >0.5% regressions elsewhere): … `optimize` without whole-`Body` clone; …
//!
//! The acceptance criterion encoded below: the optimizer exposes an
//! in-place seam [`crate::optimize_in_place`] that mutates the caller's
//! `Body` directly (no deep copy anywhere in the call), and the `Arc`-taking
//! entries consume the `Arc<Body>` **by value** so a uniquely-owned body is
//! optimized with zero whole-`Body` clones (`Arc::make_mut`
//! copy-on-write), while a shared body keeps its un-optimized original
//! intact.

use super::testutil::*;
use crate::{PassPlan, optimize_at_level, optimize_in_place};
use glyim_core::Mutability;
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;
use std::sync::Arc;

/// A body with a dead store to `_2` (never read) that DCE must remove.
fn dead_store_body(ctx: &mut glyim_type::TyCtxMut) -> Body {
    let i32_ty = ty_i32(ctx);
    let locals = vec![
        (i32_ty, Mutability::Mut), // _0 return
        (i32_ty, Mutability::Mut), // _1
        (i32_ty, Mutability::Mut), // _2 (dead)
    ];
    let block0 = make_block(
        vec![
            assign_stmt(Place::new(local(2)), const_int_rvalue(42, i32_ty)),
            assign_stmt(Place::new(local(1)), const_int_rvalue(10, i32_ty)),
            assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(1)))),
        ],
        return_term(),
    );
    build_test_body(locals, vec![block0], 0, i32_ty)
}

#[test]
fn optimize_in_place_mutates_callers_body() {
    let (ctx, mut body) = with_fresh_ty_ctx(dead_store_body);
    let plan = PassPlan::for_opt_level(2);
    optimize_in_place(&ctx, &mut body, &plan).expect("in-place optimize must succeed");
    // DCE ran directly on the caller's body: the dead store to _2 is gone.
    for stmt in &body.basic_blocks[bb(0)].statements {
        if let StatementKind::Assign(place, _) = &stmt.kind {
            assert_ne!(
                place.local,
                local(2),
                "optimize_in_place must mutate the caller's body without a whole-Body clone"
            );
        }
    }
}

#[test]
fn shared_arc_input_keeps_original_untouched() {
    let (ctx, body) = with_fresh_ty_ctx(dead_store_body);
    let arc = Arc::new(body);
    let before = (*arc).clone();
    // The Arc is shared (this clone keeps the caller's reference alive), so
    // the copy-on-write entry must deep-copy instead of mutating in place.
    let _optimized =
        optimize_at_level(&ctx, Arc::clone(&arc), 2).expect("shared-input optimize must succeed");
    assert_eq!(
        format!("{:?}", *arc),
        format!("{before:?}"),
        "a shared Arc input must keep its un-optimized original intact"
    );
}

#[test]
fn unique_arc_input_is_consumed_by_value() {
    let (ctx, body) = with_fresh_ty_ctx(dead_store_body);
    // Compile-level contract: the entry consumes the `Arc<Body>` by value,
    // so a uniquely-owned body takes the `Arc::make_mut` fast path (no
    // whole-`Body` clone) — the M2.7 quick win.
    let optimized =
        optimize_at_level(&ctx, Arc::new(body), 2).expect("owned optimize must succeed");
    assert!(!optimized.body.basic_blocks.is_empty());
    for stmt in &optimized.body.basic_blocks[bb(0)].statements {
        if let StatementKind::Assign(place, _) = &stmt.kind {
            assert_ne!(place.local, local(2), "dead store should be eliminated");
        }
    }
}
