//! HIR lowering phase (M5.1 Phase 3): lower the expanded syntax tree to HIR,
//! extracting `prepare_compilation`'s inline `Phase::Hir` block into a
//! [`CompilerPhase`]. The block is moved verbatim — the
//! `lower_crate_for_pipeline_with_file_id` call, the diagnostic accumulation,
//! and the Error-severity safety gate (an Error HIR diagnostic is fatal: the
//! async `.await`-in-a-loop guard's miscompile would otherwise fall through to
//! a hanging `loop {}` desugar) are all preserved exactly, so the diagnostic
//! set and the HIR the successor `TypeckPhase` consumes are byte-identical to
//! the monolith.
//!
//! # Why HIR lowering is its own phase
//!
//! `lower_crate_for_pipeline_with_file_id` is a single self-contained call
//! with one typed output (`CrateHir`) and one diagnostic stream. Unlike the
//! typeck block (which interleaves trait-context setup, ADT/alias/impl-header
//! registration, and `typeck_crate` across several mutation passes with no
//! clean single output) it has no cross-phase aggregates, so it lifts cleanly
//! behind the trait boundary. The `db.intern_mut()` borrow the lowerer needs
//! threads through [`PhaseCtx::db`] (`&mut Database`), so no `Option`-filled
//! -later state is introduced.
use crate::Phase;
use crate::phase::{CompilerPhase, PhaseCtx, extract_diagnostics};
use glyim_diag::{CompResult, DiagSeverity};
use glyim_hir::CrateHir;
use glyim_span::FileId;
use glyim_syntax::SyntaxNode;

/// Output of [`HirPhase`]: the lowered HIR crate.
pub struct HirOutput {
    /// The lowered HIR crate (`TypeckPhase`'s input).
    pub hir: CrateHir,
}

/// Lower the expanded syntax tree to HIR. Extracted verbatim from
/// `prepare_compilation`'s inline `Phase::Hir` block.
///
/// Carries the expanded root and the anchoring `file_id` (the latter feeds
/// `file!`/`include!`/`line!` resolution inside the lowerer). The
/// `Error`-severity gate is the async-desugar safety valve: an Error HIR
/// diagnostic means the lowered body would miscompile (e.g. an
/// `.await`-in-a-loop falling through to a `Pending` arm that is an infinite
/// `loop {}`), so the phase short-circuits rather than letting the bad HIR
/// reach typeck.
pub struct HirPhase<'a> {
    /// The macro-expanded crate root (`DefMapOutput::expanded_root`).
    pub expanded_root: &'a SyntaxNode,
    /// The file id anchoring `file!`/`include!`/`line!` in the lowerer.
    pub file_id: FileId,
}

impl CompilerPhase for HirPhase<'_> {
    type Output = HirOutput;
    fn name(&self) -> &'static str {
        "hir"
    }
    fn run(&self, ctx: &mut PhaseCtx) -> CompResult<Self::Output> {
        let (hir, hir_diags) = {
            let _guard = ctx.recorder.enter(Phase::Hir);
            glyim_hir::pipeline_api::lower_crate_for_pipeline_with_file_id(
                self.expanded_root,
                ctx.db.intern_mut(),
                self.file_id,
            )
        };
        ctx.sink.borrow_mut().extend(hir_diags.clone());
        // Safety gate: a compiler must not emit a binary when HIR lowering
        // produced an Error-severity diagnostic. Notably this makes the async
        // desugar's `.await`-inside-a-loop guard (Error 60) fatal instead of a
        // silent miscompile — the lowered body would otherwise fall through to
        // the single-poll desugar whose `Pending` arm is an infinite `loop {}`
        // that hangs if ever executed. HIR errors are always real, so fail fast.
        if hir_diags
            .iter()
            .any(|d| matches!(d.severity, DiagSeverity::Error))
        {
            return Err(extract_diagnostics(ctx));
        }
        Ok(HirOutput { hir })
    }
}
