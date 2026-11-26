//! Pass-manager tests (M5.11): verify-after-each-pass attribution, the
//! `optimize_with_bisect` limit API, and per-pass [`PassStats`].
//!
//! The bisect + attribution tests inject a `CorruptingPass` (a test-only
//! [`MirPass`] that retargets block 0's terminator to a missing block) at a
//! known index into a `&[&dyn MirPass]` slice and drive the shared
//! [`run_plan_with_verification`] core directly — that core is what
//! [`crate::optimize_with_bisect`] threads through, so the bisect result is
//! the public API's result.

use super::testutil::*;
use crate::pass_plan::{BuiltinMirPass, MirPass, PassPlan};
use crate::{OptError, PassViolation, ViolationKind, optimize_with_bisect};
use glyim_core::Mutability;
use glyim_mir::{BasicBlockIdx, Body, SourceInfo, Terminator, TerminatorKind};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use std::sync::Arc;

/// A test-only pass that corrupts the body: retargets block 0's terminator
/// to a non-existent block. The manager's verify-after-each-pass must catch
/// the resulting `UnknownTarget` violation and attribute it to this pass.
struct CorruptingPass;

impl MirPass for CorruptingPass {
    fn id(&self) -> &'static str {
        "corrupting"
    }
    fn run(&self, _ctx: &glyim_type::TyCtx, body: &mut Body) {
        // `get_mut` so the construction never panics if block 0 is absent.
        if let Some(data) = body.basic_blocks.get_mut(BasicBlockIdx::from_usize(0)) {
            data.terminator = Terminator {
                kind: TerminatorKind::Goto {
                    target: BasicBlockIdx::from_usize(999),
                },
                source_info: SourceInfo::new(Span::DUMMY),
            };
        }
    }
}

/// A minimal valid body: one block ending in `Return`, one `i32` local (the
/// return place). Every builtin pass is a no-op on it, so the only way the
/// manager fails is the injected `CorruptingPass`.
fn trivial_body() -> (glyim_type::TyCtx, Body) {
    let (ctx, i32_ty) = with_fresh_ty_ctx(ty_i32);
    let block = make_block(
        Vec::new(),
        Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    );
    let body = build_test_body(vec![(i32_ty, Mutability::Not)], vec![block], 0, i32_ty);
    (ctx, body)
}

/// M5.11 verify-after-each-pass: a pass that produces an ill-formed body is
/// attributed by name. `CorruptingPass` runs second; the manager must return
/// `PassViolated` with `pass == "corrupting"` and an `UnknownTarget`
/// violation, not a generic "optimization failed".
#[test]
fn verify_after_each_pass_attributes_violations_to_the_offending_pass() {
    let (ctx, body) = trivial_body();
    let passes: Vec<&dyn MirPass> = vec![
        &BuiltinMirPass::SliceDesugar as &dyn MirPass,
        &CorruptingPass as &dyn MirPass,
    ];
    let mut b = body.clone();
    let err = crate::run_plan_with_verification(&ctx, &mut b, &passes, None).unwrap_err();
    let OptError::PassViolated(PassViolation { pass, violations }) = err else {
        panic!("expected PassViolated, got {err:?}");
    };
    assert_eq!(pass, "corrupting");
    assert!(
        violations
            .iter()
            .any(|v| matches!(v.kind, ViolationKind::UnknownTarget(_)))
    );
}

/// M5.11 bisect: inject `CorruptingPass` at index 2 in a four-pass slice and
/// binary-search the smallest `limit` that fails. The corrupting pass runs
/// only when `limit > 2` (i.e. `limit >= 3`), so the first failing `limit`
/// is 3 and the bad pass index is `limit - 1 == 2` — the exact index we
/// injected. The error at that limit attributes to `"corrupting"`.
#[test]
fn bisect_names_the_exact_corrupting_pass_index() {
    let (ctx, body) = trivial_body();
    let passes: Vec<&dyn MirPass> = vec![
        &BuiltinMirPass::SliceDesugar as &dyn MirPass,
        &BuiltinMirPass::ConstantProp as &dyn MirPass,
        &CorruptingPass as &dyn MirPass,
        &BuiltinMirPass::Dce as &dyn MirPass,
    ];
    const CORRUPTING_INDEX: usize = 2;

    // Find the smallest limit in 0..=passes.len() at which the run fails.
    let mut first_failing_limit = None;
    for limit in 0..=passes.len() {
        let mut b = body.clone();
        if crate::run_plan_with_verification(&ctx, &mut b, &passes, Some(limit)).is_err() {
            first_failing_limit = Some(limit);
            break;
        }
    }
    let first_failing_limit = first_failing_limit.expect("some limit must fail");

    // The bad pass index is one less than the first failing limit (the limit
    // that first includes the corrupting pass).
    assert_eq!(
        first_failing_limit - 1,
        CORRUPTING_INDEX,
        "bisect must name the exact corrupting-pass index"
    );

    // And the error at that limit attributes to the corrupting pass by id.
    let mut b = body.clone();
    let err = crate::run_plan_with_verification(&ctx, &mut b, &passes, Some(first_failing_limit))
        .unwrap_err();
    let OptError::PassViolated(PassViolation { pass, .. }) = err else {
        panic!("expected PassViolated at the failing limit, got {err:?}");
    };
    assert_eq!(pass, "corrupting");
}

/// M5.11 per-pass stats: `optimize_with_bisect` records one [`PassStats`] per
/// pass in the plan, with `ran` reflecting the limit and the pass ids in run
/// order. Stats are structural (no wall-clock), so the run is deterministic.
#[test]
fn optimize_with_bisect_records_per_pass_stats() {
    let (ctx, body) = trivial_body();
    let plan = PassPlan::for_opt_level(2);
    let n = plan.passes().len();
    let (_opt, stats) = optimize_with_bisect(&ctx, Arc::new(body), &plan, n).unwrap();
    assert_eq!(stats.len(), n);
    assert!(stats.iter().all(|s| s.ran));
    assert_eq!(stats[0].name, "slice_desugar");
    assert_eq!(stats.iter().map(|s| s.name).collect::<Vec<_>>(), plan.ids());
}

/// M5.11 bisect limit: a limit below the plan length leaves the tail passes
/// `ran: false` and still succeeds on a valid body.
#[test]
fn optimize_with_bisect_limit_marks_tail_passes_not_ran() {
    let (ctx, body) = trivial_body();
    let plan = PassPlan::for_opt_level(2);
    let n = plan.passes().len();
    // Run only the first pass (slice_desugar).
    let (_opt, stats) = optimize_with_bisect(&ctx, Arc::new(body), &plan, 1).unwrap();
    assert_eq!(stats.len(), n);
    assert!(stats[0].ran);
    assert!(stats[1..].iter().all(|s| !s.ran));
}

/// `MirPass::ensures` / `requires` declarations: every builtin pass declares
/// `WellFormed` as a requirement, and only `slice_desugar` declares the
/// `NoSubslice` post-condition. The manager relies on these to drive the
/// phase-specific post-condition check.
#[test]
fn builtin_pass_declarations_are_wired() {
    use crate::pass_plan::Invariant;
    for pass in BuiltinMirPass::ALL {
        assert!(
            pass.requires().contains(&Invariant::WellFormed),
            "{} must require WellFormed",
            pass.id()
        );
    }
    let sd = BuiltinMirPass::SliceDesugar;
    assert_eq!(sd.ensures(), &[Invariant::NoSubslice]);
    for pass in [
        BuiltinMirPass::ConstantProp,
        BuiltinMirPass::Dce,
        BuiltinMirPass::CfgSimplify,
        BuiltinMirPass::UnreachableElim,
        BuiltinMirPass::DropElaboration,
    ] {
        assert!(
            pass.ensures().is_empty(),
            "{} must not declare ensures",
            pass.id()
        );
    }
}
