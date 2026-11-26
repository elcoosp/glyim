//! THIR → MIR lowering + borrow-check + optimization phase (M5.1 Phase 2):
//! the per-body loop that lowers each `thir::Body` to MIR, borrow-checks it,
//! optimizes it, and recurses into the closures lowering captured — extracted
//! verbatim from `prepare_compilation`'s inline `Phase::Lower`/`Phase::Borrowck`/
//! `Phase::Opt` block.
//!
//! # Why this stays one phase (not three)
//!
//! `CompilerPhase::run` produces one typed output per call, and the per-body
//! loop interleaves lower → borrowck → opt with a short-circuit `has_errors`
//! gate *after each borrow check* (the original monolith returned early the
//! moment a body's borrow check failed, never lowering the next body).
//! `DiagSink::into_diagnostics` sorts + dedups on a total key (file, lo, hi,
//! code, message), so the final diagnostic *set* is order-independent — but
//! the short-circuit gates which bodies get lowered at all, so a clean staged
//! split (lower-all, then borrowck-all, then opt-all) would lower bodies the
//! original never reached and surface their lowering warnings. Keeping the
//! loop intact inside one phase preserves 100% parity. Splitting borrowck/opt
//! into their own staged phases is deferred to M5.1 Phase 3, where the typed
//! `PhaseContext` hardening (no `Option`-filled-later state) lets each stage
//! carry its own typed input and the short-circuit moves onto the phase
//! boundary rather than inside one body loop.
//!
//! # What moves out of the monolith
//!
//! The `ty_ctx_after_analysis` guard, the `PipelineLowerCtx`/
//! `PipelineBorrowckCtx` construction, the `glyim_lower::lower_body` →
//! `drop_raw_generic_err_warnings` → `glyim_borrowck::check_borrows` →
//! `glyim_opt::optimize_at_level` sequence, the pre-opt body capture
//! (`--emit=mir` reads the pre-DCE image), the closure recursion (P6.2:
//! closures are real MIR bodies that must be optimized + emitted as
//! `__glyim_fn_{closure_id}`), and the `closure_body_has_unmonomorphized_ty`
//! skip (Script 640: generic stdlib closures never reach codegen). The output
//! carries the optimized body map, the pre-opt body map, and the closure
//! codegen pairs `prepare_compilation` threads into mono + the symbol pairing.
use crate::Phase;
use crate::phase::{CompilerPhase, PhaseCtx, extract_diagnostics};
use crate::pipeline_context::{PipelineBorrowckCtx, PipelineLowerCtx};
use crate::{
    closure_body_has_unmonomorphized_ty, drop_raw_generic_err_warnings, ty_ctx_after_analysis,
};
use glyim_const_eval::ConstValue;
use glyim_core::def_id::{ConstDefId, DefId, LocalDefId};
use glyim_diag::{CompResult, GlyimDiagnostic};
use glyim_hir::CrateHir;
use glyim_mir::Body;
use glyim_typeck::TypedBody;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Output of [`LowerPhase`]: the optimized MIR body map, the pre-optimization
/// body map (`--emit=mir`), the closure bodies paired with their codegen
/// symbols, and the per-pass optimization stats (M5.11).
pub struct LowerOutput {
    /// `DefId` → optimized MIR body, ready for monomorphization.
    pub mir_bodies_map: BTreeMap<DefId, Arc<Body>>,
    /// `DefId` → the same bodies *before* `glyim_opt::optimize`, so
    /// `--emit=mir` shows lowering artifacts DCE would otherwise erase.
    pub pre_opt_bodies_map: HashMap<DefId, Arc<Body>>,
    /// Per-pass optimization stats, one row per planned pass per optimized
    /// body (M5.11 `--opt-bisect-limit` observability). Populated only when
    /// the compile runs bisected (`LowerPhase::opt_bisect_limit` is
    /// `Some`); empty otherwise, so the default path pays nothing. Rows are
    /// appended in the per-body loop order, which is the deterministic
    /// `typed_bodies` order, so the aggregation the CLI prints under
    /// `--time-passes` is reproducible across runs (C6).
    pub pass_stats: Vec<(DefId, glyim_opt::PassStats)>,
    /// Closure bodies captured while lowering, each paired with its
    /// `__glyim_fn_{closure_id}` codegen symbol.
    pub closure_bodies_codegen: Vec<(String, Arc<Body>)>,
}

/// Lower every type-checked THIR body to MIR, borrow-check it, optimize it,
/// and recurse into captured closures. Extracted verbatim from
/// `prepare_compilation`'s inline `Phase::Lower`/`Phase::Borrowck`/`Phase::Opt`
/// block so the per-body short-circuit (`has_errors` after each borrow check)
/// is preserved exactly.
pub struct LowerPhase<'a> {
    /// The verified THIR bodies (`typeck_result.typed_bodies`, plan M5.2):
    /// lowering accepts only `TypedBody`, so a body whose signature slots
    /// still carry an inference variable or the `Ty::ERROR` sentinel never
    /// reaches this phase.
    pub typed_bodies: &'a [(LocalDefId, TypedBody)],
    /// The lowered HIR crate.
    pub hir: &'a CrateHir,
    /// Const values resolved by typeck (`typeck_result.const_values`).
    pub const_values: &'a HashMap<ConstDefId, ConstValue>,
    /// The caller's `--opt-level`, threaded into `glyim_opt::optimize_at_level`.
    pub opt_level: u8,
    /// The caller's `--opt-bisect-limit=N` (M5.11). `Some(n)` runs only the
    /// first `n` passes of [`glyim_opt::PassPlan::for_opt_level`] via
    /// `glyim_opt::optimize_with_bisect` and collects one
    /// [`glyim_opt::PassStats`] row per planned pass; `None` keeps the
    /// plain `optimize_at_level` path (no stats collected, no bisect).
    pub opt_bisect_limit: Option<usize>,
}

impl CompilerPhase for LowerPhase<'_> {
    type Output = LowerOutput;
    fn name(&self) -> &'static str {
        "lower-borrowck-opt"
    }
    fn run(&self, ctx: &mut PhaseCtx) -> CompResult<Self::Output> {
        let mut mir_bodies_map: BTreeMap<DefId, Arc<Body>> = BTreeMap::new();
        let mut pre_opt_bodies_map: HashMap<DefId, Arc<Body>> = HashMap::new();
        let mut closure_bodies_codegen: Vec<(String, Arc<Body>)> = Vec::new();
        let mut pass_stats: Vec<(DefId, glyim_opt::PassStats)> = Vec::new();

        let ty_ctx_guard = ty_ctx_after_analysis(ctx.db, "lowering")?;
        let ty_ctx_ref = ty_ctx_guard.as_ref();

        let lower_ctx = PipelineLowerCtx::new(ty_ctx_ref, self.hir, self.const_values.clone());

        for (_owner_def_id, typed_body) in self.typed_bodies {
            let mut lower_result = {
                let _guard = ctx.recorder.enter(Phase::Lower);
                glyim_lower::lower_typed_body(&lower_ctx, typed_body)
            };
            // A never-instantiated generic template's placeholder `Err`
            // nodes are expected; drop only that warning for such bodies.
            drop_raw_generic_err_warnings(
                &mut lower_result.diagnostics,
                &lower_result.body,
                ty_ctx_ref,
            );
            ctx.sink.borrow_mut().extend(lower_result.diagnostics);
            if ctx.sink.borrow().has_errors() {
                return Err(extract_diagnostics(ctx));
            }
            let mir_body = lower_result.body;
            let owner = mir_body.owner;
            let mir_arc = Arc::new(mir_body);

            let borrowck_ctx = PipelineBorrowckCtx::new(ty_ctx_ref, &mir_arc);
            let borrowck_result = {
                let _guard = ctx.recorder.enter(Phase::Borrowck);
                glyim_borrowck::check_borrows(&borrowck_ctx, &mir_arc)
            };
            ctx.sink.borrow_mut().extend(borrowck_result.errors);
            if ctx.sink.borrow().has_errors() {
                return Err(extract_diagnostics(ctx));
            }

            // Capture the pre-optimization body so `--emit=mir` can
            // show the lowering artifacts (DCE would otherwise erase
            // writes to dead locals, which the compound-assign
            // regression test relies on seeing).
            pre_opt_bodies_map.insert(owner, mir_arc.clone());

            // M5.11: with `--opt-bisect-limit` the plan runs bisected and
            // collects one `PassStats` row per planned pass; without it the
            // plain path runs (no stats, no collection cost).
            let (opt_body, body_stats) = {
                let _guard = ctx.recorder.enter(Phase::Opt);
                match self.opt_bisect_limit {
                    Some(limit) => {
                        let plan = glyim_opt::PassPlan::for_opt_level(self.opt_level);
                        glyim_opt::optimize_with_bisect(ty_ctx_ref, mir_arc, &plan, limit)
                    }
                    None => glyim_opt::optimize_at_level(ty_ctx_ref, mir_arc, self.opt_level)
                        .map(|optimized| (optimized, Vec::new())),
                }
            }
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "MIR optimization failed for {owner:?}: {e:?}"
                ))]
            })?;
            pass_stats.extend(body_stats.into_iter().map(|s| (owner, s)));
            mir_bodies_map.insert(owner, Arc::new(opt_body.body));

            // P6.2: closures captured while lowering this function are real
            // MIR bodies that must also be optimized, registered, and emitted
            // as `__glyim_fn_{closure_id}` by codegen.
            for (_cid, _substs, cbody) in lower_result.closure_bodies {
                // Script 640: skip closures whose locals still reference
                // an un-resolved type. These come from *generic* stdlib
                // definitions lowered in bulk; they are never
                // monomorphized because `main` never reaches them, but
                // pushing them unconditionally aborts codegen
                // (`fn_abi_of` → `UnknownType`). A reachable closure
                // always arrives here with concrete locals.
                if closure_body_has_unmonomorphized_ty(&cbody, ty_ctx_ref) {
                    continue;
                }
                let c_mir_arc = Arc::new(cbody);
                let c_borrowck_ctx = PipelineBorrowckCtx::new(ty_ctx_ref, &c_mir_arc);
                let c_borrowck_result = {
                    let _guard = ctx.recorder.enter(Phase::Borrowck);
                    glyim_borrowck::check_borrows(&c_borrowck_ctx, &c_mir_arc)
                };
                ctx.sink.borrow_mut().extend(c_borrowck_result.errors);
                if ctx.sink.borrow().has_errors() {
                    return Err(extract_diagnostics(ctx));
                }
                let c_owner = c_mir_arc.owner;
                let (c_opt, c_stats) = {
                    let _guard = ctx.recorder.enter(Phase::Opt);
                    match self.opt_bisect_limit {
                        Some(limit) => {
                            let plan = glyim_opt::PassPlan::for_opt_level(self.opt_level);
                            glyim_opt::optimize_with_bisect(ty_ctx_ref, c_mir_arc, &plan, limit)
                        }
                        None => glyim_opt::optimize_at_level(ty_ctx_ref, c_mir_arc, self.opt_level)
                            .map(|optimized| (optimized, Vec::new())),
                    }
                }
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "MIR optimization failed for closure body: {e:?}"
                    ))]
                })?;
                pass_stats.extend(c_stats.into_iter().map(|s| (c_owner, s)));
                let c_body = c_opt.body;
                let c_sym = format!("__glyim_fn_{}", c_body.owner.local_id.to_raw());
                closure_bodies_codegen.push((c_sym, Arc::new(c_body)));
            }
        }

        Ok(LowerOutput {
            mir_bodies_map,
            pre_opt_bodies_map,
            pass_stats,
            closure_bodies_codegen,
        })
    }
}
