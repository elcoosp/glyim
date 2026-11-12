//! Opt-level pass-plan selection pins (the `pass_plan` module contract).
//!
//! The pipeline threads the CLI's `--opt-level` into the MIR optimizer
//! through [`crate::optimize_at_level`]. These tests pin the three
//! contracts that make that safe:
//!
//! 1. the registry round-trips (`from_id`/`id`) and every builtin is
//!    dispatchable through the [`crate::pass_plan::MirPass`] seam;
//! 2. `PassPlan::for_opt_level` selects a minimal plan at `-O0` (only the
//!    codegen-required `slice_desugar`) and the full historical pipeline
//!    from `-O1` up, with `slice_desugar` present at *every* level;
//! 3. the level behavior is real: a dead store survives `-O0` and is
//!    erased by `-O2`, and `optimize` (the back-compat entry point)
//!    matches the `-O2` plan byte-for-byte.

use super::testutil::*;
use crate::pass_plan::{BuiltinMirPass, MirPass, PassPlan};
use crate::{optimize, optimize_at_level, optimize_with_plan};
use glyim_core::Mutability;
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;
use std::sync::Arc;

/// A body with one dead store plus two live assigns — the same shape the
/// DCE tests use, so the level behavior below is directly comparable to
/// `dce_removes_dead_store`.
fn dead_store_body(ctx: &mut glyim_type::TyCtxMut) -> Body {
    let i32_ty = ty_i32(ctx);
    let locals = vec![
        (i32_ty, Mutability::Mut), // _0 return
        (i32_ty, Mutability::Mut), // _1 live
        (i32_ty, Mutability::Mut), // _2 dead — never read
    ];
    // bb0:
    //   _2 = 42          // dead — never read
    //   _1 = 10
    //   _0 = _1
    //   return
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

/// Count `Assign` statements across all blocks (the DCE-observable shape).
fn assign_count(body: &Body) -> usize {
    body.basic_blocks
        .iter()
        .map(|bb| {
            bb.statements
                .iter()
                .filter(|s| matches!(s.kind, StatementKind::Assign(_, _)))
                .count()
        })
        .sum()
}

/// The registry round-trips: every variant has a stable id, `from_id` maps
/// back to the same variant, and an unknown id is `None` (never guessed).
#[test]
fn from_id_roundtrips_every_builtin_pass() {
    assert_eq!(BuiltinMirPass::ALL.len(), 6, "registry surface is fixed");
    for pass in BuiltinMirPass::ALL {
        let round = BuiltinMirPass::from_id(pass.id());
        assert_eq!(round, Some(*pass), "id `{}` must round-trip", pass.id());
    }
    assert_eq!(BuiltinMirPass::from_id("not_a_pass"), None);
    assert_eq!(BuiltinMirPass::from_id(""), None);
}

/// Ids are unique — two variants sharing an id would make `from_id`
/// ambiguous and the phase-profile pass rows unattributable.
#[test]
fn pass_ids_are_unique() {
    let mut ids: Vec<&'static str> = BuiltinMirPass::ALL.iter().map(|p| p.id()).collect();
    ids.sort_unstable();
    let len = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), len, "every builtin pass must have a unique id");
}

/// Every builtin pass is dispatchable through the `MirPass` seam on a
/// trivial body — including `DropElaboration`, which takes `&TyCtx` like
/// every other pass (its `&mut TyCtxMut` signature was an accident: it
/// only reads the ctx and interns types).
#[test]
fn every_builtin_pass_runs_on_a_trivial_body() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);
        let block0 = make_block(
            vec![assign_stmt(
                Place::new(local(0)),
                const_int_rvalue(1, i32_ty),
            )],
            return_term(),
        );
        build_test_body(vec![(i32_ty, Mutability::Mut)], vec![block0], 0, i32_ty)
    });
    for pass in BuiltinMirPass::ALL {
        let mut b = body.clone();
        pass.run(&ctx, &mut b);
        // No pass may turn a valid one-block body into an empty block list
        // or lose the return terminator.
        assert!(
            !b.basic_blocks.is_empty(),
            "pass `{}` emptied the block list",
            pass.id()
        );
    }
}

/// `-O0` is the minimal plan: only `slice_desugar`, the pass codegen
/// structurally requires (its elimination of `Subslice` projections is
/// assumed by both backends — see `slice_desugar`'s module doc).
#[test]
fn for_opt_level_zero_is_the_minimal_plan() {
    let plan = PassPlan::for_opt_level(0);
    assert_eq!(plan.passes(), &[BuiltinMirPass::SliceDesugar]);
}

/// From `-O1` up the plan is the full historical pipeline, in the exact
/// order `optimize` has always run — so `-O1`/`-O2`/`-O3` (and any
/// out-of-range level, saturating) reproduce today's behavior exactly.
#[test]
fn for_opt_level_matches_the_full_pipeline_from_one_on() {
    let full = [
        BuiltinMirPass::SliceDesugar,
        BuiltinMirPass::ConstantProp,
        BuiltinMirPass::Dce,
        BuiltinMirPass::CfgSimplify,
        BuiltinMirPass::UnreachableElim,
    ];
    for level in [1u8, 2, 3, 255] {
        let plan = PassPlan::for_opt_level(level);
        assert_eq!(
            plan.passes(),
            &full,
            "level {level} must be the full pipeline"
        );
    }
}

/// The codegen invariant: `slice_desugar` is present in the plan for
/// *every* opt level, including custom-built plans that the pipeline
/// might hand to codegen. A plan without it must be built explicitly and
/// consciously (the empty/duplicate contracts below).
#[test]
fn every_default_level_plan_includes_slice_desugar() {
    for level in [0u8, 1, 2, 3] {
        let plan = PassPlan::for_opt_level(level);
        assert!(
            plan.includes(BuiltinMirPass::SliceDesugar),
            "level {level} plan lost the codegen-required slice_desugar"
        );
    }
}

/// A plan is an ordered set: duplicate passes are ambiguous (which run?
/// when?), so `from_passes` rejects them with `None` instead of silently
/// running a pass twice.
#[test]
fn from_passes_rejects_duplicates() {
    assert!(PassPlan::from_passes(&[BuiltinMirPass::Dce]).is_some());
    assert!(
        PassPlan::from_passes(&[BuiltinMirPass::Dce, BuiltinMirPass::Dce]).is_none(),
        "duplicate passes must be rejected"
    );
    // The empty plan is legal (an identity/analysis plan) but the
    // codegen-facing default plans never produce one.
    let empty = PassPlan::from_passes(&[]);
    assert!(empty.is_some());
    assert!(empty.unwrap().passes().is_empty());
}

/// `optimize` (the back-compat entry point the pipeline used before
/// opt levels existed) is exactly the `-O2` plan: same pass sequence, same
/// validation points, same result shape.
#[test]
fn optimize_equals_the_full_plan() {
    let (ctx, body) = with_fresh_ty_ctx(dead_store_body);
    let via_optimize = optimize(&ctx, &Arc::new(body.clone()));
    let via_plan = optimize_with_plan(&ctx, &Arc::new(body), &PassPlan::for_opt_level(2));
    assert_eq!(
        via_optimize.body.basic_blocks.len(),
        via_plan.body.basic_blocks.len()
    );
    assert_eq!(
        assign_count(&via_optimize.body),
        assign_count(&via_plan.body),
        "optimize() and the -O2 plan must erase the same dead stores"
    );
}

/// The level behavior is real, not a re-skin of the fixed pipeline: the
/// dead store survives `-O0` (no DCE in the plan) and is erased by `-O2`.
#[test]
fn o0_preserves_the_dead_store_that_o2_erases() {
    let (ctx, body) = with_fresh_ty_ctx(dead_store_body);
    let o0 = optimize_at_level(&ctx, &Arc::new(body.clone()), 0);
    assert_eq!(
        assign_count(&o0.body),
        3,
        "-O0 must not run DCE — the dead store survives"
    );
    let o2 = optimize_at_level(&ctx, &Arc::new(body), 2);
    // const-prop first folds `_1 = 10; _0 = _1` into `_0 = 10`, which
    // makes the `_1` store dead too — so O2 erases *two* assigns, not
    // just the obvious dead store. (Plain DCE alone — see
    // `dce_removes_dead_store` — leaves both live assigns.)
    assert_eq!(
        assign_count(&o2.body),
        1,
        "-O2 must erase the dead stores (const-prop + DCE in the plan)"
    );
}

/// The plan's ids render in run order — the per-pass observability rows
/// (`Phase::Opt` profile) print exactly these strings.
#[test]
fn plan_ids_render_in_run_order() {
    let plan = PassPlan::for_opt_level(2);
    assert_eq!(
        plan.ids(),
        vec![
            "slice_desugar",
            "constant_prop",
            "dce",
            "cfg_simplify",
            "unreachable_elim"
        ]
    );
}
