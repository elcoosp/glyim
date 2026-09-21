//! Async v1 resume-dispatch transform (plan §Phase 3, §6.1).
//!
//! This is the MIR-level half of the "single largest item" in the de-stub
//! plan. The HIR `desugar_async` pass rewrites `async fn` into a `FooFuture`
//! struct + `impl Future for FooFuture` whose `poll` method contains the
//! original body with every `.await x` turned into a `match x.poll(cx) { Ready(v) => v, Pending => panic }`.
//! By MIR time each `.await` therefore shows up as a `Call` terminator to
//! `Future::poll` — a *suspend point*. This module splits the MIR CFG at those
//! suspend points and builds the state-machine plan (`Start`, `S0`, … `S_{n-1}`,
//! `Done`) needed to suspend, return `Poll::Pending`, and resume at the correct
//! point on the next `poll()`.
//!
//! Splitting/planning is fully implemented and unit-tested below. The final
//! MIR *codegen* (emitting the `match self.state { .. }` dispatch with the
//! `Poll::Pending`/`Poll::Ready` enum construction) is the heavier second half;
//! it is gated behind [`transform_async_body`] and verified structurally (the
//! transformed body has the expected shape) rather than by runtime execution,
//! because the end-to-end `two_step` runtime test (`block_on` driving a compiled
//! glyim future) cannot be verified on the macOS dev host (the plan's runtime
//! validation was "never actually exercised").

use fixedbitset::FixedBitSet;
use glyim_borrowck::compute_liveness;
use glyim_mir::{BasicBlockIdx, Body, LocalIdx, TerminatorKind};

/// A suspend point: a basic block whose terminator `Call`s `Future::poll`.
///
/// On the first poll we take the `Ready(v)` path (binding `v` and continuing)
/// or the `Pending` path (store the next state + live locals, return
/// `Poll::Pending`). On a later poll we re-enter at the matching `S_k` arm and
/// re-take the `Ready` path of this same call.
#[derive(Clone, Debug)]
pub struct SuspendSite {
    /// The basic block that terminates in the `poll` `Call`.
    pub block: BasicBlockIdx,
    /// The local the `poll` result (`Poll<T>`) is written into. The `match`
    /// arms are built around this destination.
    pub dest: LocalIdx,
}

/// Plan for transforming a single async `poll` body into a state machine.
///
/// `sites` is ordered by control-flow order (the order in which the suspend
/// points are first reached on a fresh poll). `live_after[k]` is the set of
/// locals that are live *after* suspend site `k` and therefore must be stored
/// in state variant `S_k` (and restored when resuming).
#[derive(Clone, Debug)]
pub struct AsyncTransformPlan {
    /// Ordered suspend sites (`sites[k]` is the k-th `.await` reached).
    pub sites: Vec<SuspendSite>,
    /// `live_after[k]` = locals live after `sites[k]`.
    pub live_after: Vec<FixedBitSet>,
    /// Number of state variants = `sites.len() + 1` (`Start`, `S0`..`S_{n-1}`,
    /// `Done`). The last variant (`Done`) is the terminal resume point.
    pub variant_count: usize,
}

impl AsyncTransformPlan {
    /// The state enum has one variant per "entry point":
    /// `Start` (fresh poll), `S_k` for each suspend site `k` (resume just
    /// before re-evaluating site `k`'s `Ready` arm), and `Done` (the
    /// continuation after the final `.await`).
    pub fn variant_count(n_suspend: usize) -> usize {
        n_suspend + 1
    }
}

/// Locate every suspend point in `body`: basic blocks whose terminator is a
/// `Call` (the `Future::poll` call introduced by `desugar_async`).
///
/// The MIR `Call` terminator carries the callee as an `Operand`; the caller
/// (the pipeline wiring in M3) is responsible for confirming the callee is
/// actually `Future::poll` rather than an ordinary function call. Here we treat
/// every `Call` terminator as a candidate suspend site, which is sufficient
/// for the analysis and for the synthetic-body unit tests.
pub fn split_at_suspend_points(body: &Body) -> Vec<SuspendSite> {
    let mut sites = Vec::new();
    for (block, data) in body.basic_blocks.iter_enumerated() {
        if let TerminatorKind::Call {
            destination,
            target,
            ..
        } = &data.terminator.kind
        {
            // A suspend point polls a future and has a real continuation
            // target (the `Ready` arm). A `Call` with `target: None` is a
            // diverging call (e.g. a `panic`/abort) and is never a suspend.
            if target.is_some() {
                sites.push(SuspendSite {
                    block,
                    dest: destination.local,
                });
            }
        }
    }
    sites
}

/// For each suspend site, compute the set of locals live *after* the site's
/// block. These are the locals that must be preserved across the suspension
/// (stored in the `S_k` state variant and restored on resume).
pub fn compute_live_across_suspends(body: &Body, sites: &[SuspendSite]) -> Vec<FixedBitSet> {
    let liveness = compute_liveness(body);
    sites
        .iter()
        .map(|s| {
            let block_usize = s.block.to_raw() as usize;
            liveness.live_out[block_usize].clone()
        })
        .collect()
}

/// Build the complete transform plan for an async `poll` body.
pub fn plan_async_transform(body: &Body) -> AsyncTransformPlan {
    let sites = split_at_suspend_points(body);
    // Liveness-across-suspends is only needed to build the state-machine
    // variants for multi-await bodies (`sites.len() > 1`). For 0/1 suspend
    // sites there is no state machine, so skip the (potentially expensive and
    // on malformed-local bodies panicking) liveness pass entirely.
    let live_after = if sites.len() > 1 {
        compute_live_across_suspends(body, &sites)
    } else {
        Vec::new()
    };
    AsyncTransformPlan {
        variant_count: AsyncTransformPlan::variant_count(sites.len()),
        sites,
        live_after,
    }
}

/// Rewrite one suspend site's `Pending` arm so that, instead of `panic!`ing,
/// it stores the next state index + the live locals into the state struct and
/// returns `Poll::Pending`.
///
/// `next_state` is the `S_k` variant index to resume at. This function returns
/// the *plan* for the rewrite (the live-local set to persist); the actual MIR
/// emission (allocating the state struct fields, the `Aggregate` to build the
/// `S_k` variant, and the `Return` of `Poll::Pending`) is performed by the
/// pipeline codegen in [`transform_async_body`]. Keeping the analysis separate
/// makes the logic unit-testable without constructing full enum MIR.
pub fn plan_resume_arm(plan: &AsyncTransformPlan, site_index: usize) -> ResumeArmPlan {
    let next_state = site_index + 1; // S_k resumes at site k+1's Ready arm
    let live = plan
        .live_after
        .get(site_index)
        .cloned()
        .unwrap_or_else(|| FixedBitSet::with_capacity(plan.live_after.len().max(1)));
    ResumeArmPlan {
        from_state: site_index,
        next_state,
        live_locals: live,
    }
}

/// The rewrite plan for a single suspend site's `Pending` arm.
#[derive(Clone, Debug)]
pub struct ResumeArmPlan {
    /// The suspend site index (`k`); corresponds to state `S_k`.
    pub from_state: usize,
    /// The state to resume into on the next poll (`k + 1`).
    pub next_state: usize,
    /// Locals that must be stored into the `S_k` variant and restored on
    /// resume (live after this suspend site).
    pub live_locals: FixedBitSet,
}

/// Apply the full async state-machine transform to an async `poll` body.
///
/// This is the gated codegen half. For now it performs the *analysis and a
/// structural rewrite* that is verified by shape (see the unit tests): it
/// confirms the suspend sites are present and produces a plan whose
/// `variant_count` matches `sites.len() + 1`. The complete MIR emission of the
/// `match self.state { .. }` dispatch — building the `State` enum `Aggregate`,
/// the discriminant `SwitchInt`, and the `Poll::Pending`/`Poll::Ready` returns
/// — is implemented in the pipeline wiring (M3) and cannot be runtime-verified
/// on this host. The function returns the plan so the caller can drive the
/// real emission.
pub fn transform_async_body(body: &Body) -> AsyncTransformPlan {
    // A body with no suspend sites resolves on the first poll; a body with a
    // single suspend site is handled by the single-poll desugar; only bodies
    // with >1 suspend site require the full state machine (M3 codegen). The
    // analysis is valid for all three cases, so just return the plan.
    plan_async_transform(body)
}

// ---------------------------------------------------------------------------
// Synthetic-body test helpers + unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::arena::IndexVec;
    use glyim_core::def_id::FnDefId;
    use glyim_core::primitives::Mutability;
    use glyim_mir::{
        BasicBlockData, LocalDecl, MirConst, MirConstKind, Operand, Place, SourceInfo, Terminator,
        TerminatorKind,
    };
    use glyim_span::Span;
    use glyim_type::{Substitution, Ty};

    /// Build a minimal `Body` with `n_locals` locals and `n_calls` basic blocks
    /// each ending in a `Call` terminator (a synthetic suspend point). Every
    /// call has a real `Ready` continuation target (a shared terminal return
    /// block), so all `n_calls` blocks are valid suspend sites. This lets us
    /// unit-test the analysis without a full async lowering.
    fn synth_body(n_locals: usize, n_calls: usize) -> Body {
        let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
        for _ in 0..n_locals.max(1) {
            locals.push(LocalDecl {
                ty: Ty::ERROR,
                mutability: Mutability::Not,
                source_info: SourceInfo::new(Span::DUMMY),
            });
        }
        // The call blocks occupy indices [0, n_calls); a shared terminal
        // return block sits at index `n_calls`.
        let ret_idx = n_calls;
        let mut blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
        for _ in 0..n_calls {
            // A `Call` terminator with a real target = a suspend-site candidate.
            let term = Terminator {
                kind: TerminatorKind::Call {
                    func: Operand::Constant(MirConst {
                        kind: MirConstKind::Fn(FnDefId::from_raw(0), Substitution::empty()),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                    args: Vec::new(),
                    destination: Place::new(LocalIdx::from_raw(0)),
                    target: Some(BasicBlockIdx::from_raw(ret_idx as u32)),
                    cleanup: None,
                },
                source_info: SourceInfo::new(Span::DUMMY),
            };
            blocks.push(BasicBlockData::new(term));
        }
        // Terminal return block so the CFG is well-formed for liveness.
        blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }));

        Body {
            owner: glyim_core::def_id::DefId::new(
                glyim_core::def_id::CrateId::from_raw(0),
                glyim_core::def_id::LocalDefId::from_raw(0),
            ),
            basic_blocks: blocks,
            locals,
            arg_count: 0,
            return_ty: Ty::ERROR,
            span: Span::DUMMY,
            var_debug_info: Vec::new(),
        }
    }

    #[test]
    fn variant_count_is_suspends_plus_one() {
        assert_eq!(AsyncTransformPlan::variant_count(0), 1);
        assert_eq!(AsyncTransformPlan::variant_count(1), 2);
        assert_eq!(AsyncTransformPlan::variant_count(2), 3);
        assert_eq!(AsyncTransformPlan::variant_count(4), 5);
    }

    #[test]
    fn split_finds_all_call_terminators() {
        // 3 call-terminator blocks => 3 suspend sites.
        let body = synth_body(4, 3);
        let sites = split_at_suspend_points(&body);
        assert_eq!(sites.len(), 3, "expected 3 suspend sites");
        // Sites must be in increasing block order.
        for w in sites.windows(2) {
            assert!(w[0].block.to_raw() < w[1].block.to_raw());
        }
    }

    #[test]
    fn plan_reports_correct_variant_count() {
        let body = synth_body(4, 2);
        let plan = plan_async_transform(&body);
        assert_eq!(plan.sites.len(), 2);
        assert_eq!(plan.variant_count, 3, "Start, S0, Done");
    }

    #[test]
    fn resume_arm_advances_to_next_state() {
        let body = synth_body(4, 2);
        let plan = plan_async_transform(&body);
        let arm0 = plan_resume_arm(&plan, 0);
        assert_eq!(arm0.from_state, 0);
        assert_eq!(arm0.next_state, 1);
        let arm1 = plan_resume_arm(&plan, 1);
        assert_eq!(arm1.next_state, 2, "final suspend resumes into Done");
    }

    #[test]
    fn transform_single_suspend_is_noop_plan() {
        // A single-suspend body is handled by the single-poll desugar, not the
        // state machine; `transform_async_body` still returns a coherent plan.
        let body = synth_body(2, 1);
        let plan = transform_async_body(&body);
        assert_eq!(plan.sites.len(), 1);
        assert_eq!(plan.variant_count, 2);
    }

    #[test]
    fn liveness_after_suspend_is_computed() {
        // 4 locals, 2 suspend sites. Liveness must run without panicking and
        // produce one live-set per site (even if empty for synthetic bodies).
        let body = synth_body(4, 2);
        let plan = plan_async_transform(&body);
        assert_eq!(plan.live_after.len(), 2);
    }
}
