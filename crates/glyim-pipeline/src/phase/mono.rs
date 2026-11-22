//! Monomorphization phase (M5.1 Phase 2): discover mono roots, collect the
//! monomorphization set, run polymorphization dedup, and the post-mono
//! semantic checks — extracted verbatim from `prepare_compilation`'s inline
//! `Phase::Mono` block.
//!
//! # Why
//!
//! The mono block is the last contiguous, single-concern slice of the
//! monolith before the CGU partitioning + symbol pairing tail. Moving it
//! behind a [`CompilerPhase`] keeps `prepare_compilation` a thin chain of
//! phase calls (lex-parse → expand-def-map → … → lower-borrowck-opt → mono)
//! and gives the mono stage a typed input (`MonoPhase`) + typed output
//! (`MonoOutput`) the M6 query engine will later re-express as the
//! `mono`/`cgu` query family (M6.6). The block is moved verbatim — the
//! discovery → collect → polymorphize → post-mono-checks ordering, the
//! per-lint `UnusedGenericParams` gating (§3.5.30), and the `has_errors`
//! short-circuit after the post-mono checks are all preserved exactly, so
//! the diagnostic set is byte-identical to the inline original.
//!
//! # What stays inline
//!
//! The annotation-needed (E0282) emission that *follows* mono stays in
//! `prepare_compilation`: it needs both `mono_items` and
//! `typeck_result.unresolved_ty_var_spans`, so it is a cross-phase
//! aggregate rather than part of the mono stage itself. The CGU partition
//! and symbol pairing tail likewise stays inline (it consumes `mono_items`,
//! `mir_bodies_map`, and `closure_bodies_codegen` from three different
//! phases).
use crate::Phase;
use crate::mono_cache::{make_drop_glue_provider, make_mir_body_provider};
use crate::phase::{CompilerPhase, PhaseCtx, extract_diagnostics};
use crate::ty_ctx_after_analysis;
use glyim_core::def_id::DefId;
use glyim_def_map::CrateDefMap;
use glyim_diag::CompResult;
use glyim_hir::CrateHir;
use glyim_lower::mono::{MonoCtx, MonoItemData};
use glyim_lower::post_mono_checks::{
    check_large_mono_set, check_unresolved_virtual_methods, check_unsized_locals,
    check_unused_generic_params,
};
use glyim_mir::Body;
use glyim_syntax::SyntaxNode;
use std::collections::HashMap;
use std::sync::Arc;

/// Output of [`MonoPhase`]: the monomorphized + deduplicated item set, ready
/// for the CGU partition + symbol pairing tail.
pub struct MonoOutput {
    /// The mono item set (`MonoCtx::items`), post-polymorphization-dedup and
    /// post-mono semantic checks.
    pub mono_items: Vec<MonoItemData>,
}

/// Discover mono roots, collect + polymorphize-dedup the mono set, and run
/// the post-monomorphization semantic checks. Extracted verbatim from
/// `prepare_compilation`'s inline `Phase::Mono` block.
pub struct MonoPhase<'a> {
    /// The parse root (`parse_result.root`) — mono-root discovery walks it.
    pub parse_root: &'a SyntaxNode,
    /// The lowered HIR crate.
    pub hir: &'a CrateHir,
    /// The crate definition map (module scope + `def_to_module`).
    pub def_map: &'a CrateDefMap,
    /// The optimized MIR body map produced by [`super::lower::LowerPhase`].
    pub mir_bodies_map: &'a HashMap<DefId, Arc<Body>>,
}

impl CompilerPhase for MonoPhase<'_> {
    type Output = MonoOutput;
    fn name(&self) -> &'static str {
        "mono"
    }
    fn run(&self, ctx: &mut PhaseCtx) -> CompResult<Self::Output> {
        let (mono_roots, discovery_diags) = {
            let _guard = ctx.recorder.enter(Phase::Mono);
            let mut ty_ctx_mut_for_discovery = glyim_type::TyCtxMut::new(ctx.db.interner().clone());
            glyim_lower::discovery::discover_mono_roots(
                self.parse_root,
                self.hir,
                Some(self.def_map),
                &mut ty_ctx_mut_for_discovery,
            )
        };
        ctx.sink.borrow_mut().extend(discovery_diags);

        let mono_items: Vec<MonoItemData> = {
            let _guard = ctx.recorder.enter(Phase::Mono);
            let mut mono_ctx = MonoCtx::new();
            let ty_ctx_guard = ty_ctx_after_analysis(ctx.db, "monomorphization")?;
            let ty_ctx_ref = ty_ctx_guard.as_ref();
            let body_provider = make_mir_body_provider(self.mir_bodies_map, ctx.sink, ty_ctx_ref);
            let drop_provider = make_drop_glue_provider(ty_ctx_ref);
            mono_ctx.with_ty_ctx(ty_ctx_ref);
            // T124-PATCHED [PIPE-4]: supply the def-id-to-module map so
            // each MonoItemData::source_module reflects its declaring
            // module. Converts ModuleId → u32 for the MonoCtx API.
            let def_to_module_u32: HashMap<glyim_core::def_id::LocalDefId, u32> = self
                .def_map
                .def_to_module
                .iter()
                .map(|(k, v)| (*k, v.to_raw()))
                .collect();
            mono_ctx.with_def_to_module(&def_to_module_u32);
            // T101-PATCHED [LOW-10]: supply the *pre-substitution* body
            // map so polymorphization can see `TyKind::Param`. The bodies
            // held by `MonoItemData` are post-substitution, so analyzing
            // them always reports the used-params set as all-false and
            // dedup would merge every instantiation of a generic fn.
            //
            mono_ctx.with_pre_subst_bodies(self.mir_bodies_map);
            mono_ctx.collect(&mono_roots, &body_provider, &drop_provider);
            // T101-PATCHED [LOW-10]: run polymorphization deduplication.
            // With the pre-subst map available, this collapses distinct
            // instantiations of the same generic fn that differ only in
            // *unused* parameters. Enabled now that T063/T141's stdlib
            // typeck regressions are fixed (manual loops replaced the
            // unregistered builtins).
            {
                let mut poly_ctx = ty_ctx_ref.to_mut();
                mono_ctx.polymorphize_and_deduplicate(&mut poly_ctx);
            }
            // §8.12: post-monomorphization semantic checks. These were dead
            // `` functions; they now run here, immediately
            // after monomorphization, so unsized locals / unused generic params
            // / pathological mono-set sizes surface as diagnostics instead of
            // being silently ignored.
            let mut post_diags = check_unsized_locals(mono_ctx.items(), ty_ctx_ref);
            post_diags.extend(check_unresolved_virtual_methods(
                mono_ctx.items(),
                ty_ctx_ref,
            ));
            // §3.5.30 framework: gate the unused-generic-params post-mono check
            // on the lint level. The check always runs (cheap); we filter
            // its output here based on `LintId::UnusedGenericParams`.
            let unused_generic_diags = check_unused_generic_params(
                mono_ctx.items(),
                Some(self.mir_bodies_map),
                ty_ctx_ref,
            );
            match ctx
                .db
                .lints()
                .get(&glyim_core::lint::LintId::UnusedGenericParams)
                .copied()
                .unwrap_or(glyim_core::lint::LintLevel::Warn)
            {
                glyim_core::lint::LintLevel::Allow => {}
                glyim_core::lint::LintLevel::Warn => {
                    post_diags.extend(unused_generic_diags);
                }
                glyim_core::lint::LintLevel::Deny => {
                    for d in unused_generic_diags {
                        let primary = d.span.primary;
                        let msg = d.message.clone();
                        post_diags.push(glyim_diag::GlyimDiagnostic::type_error(primary, msg));
                    }
                }
            }
            post_diags.extend(check_large_mono_set(mono_ctx.items(), 1000));
            ctx.sink.borrow_mut().extend(post_diags);
            mono_ctx.items().to_vec()
        };

        if ctx.sink.borrow().has_errors() {
            return Err(extract_diagnostics(ctx));
        }

        Ok(MonoOutput { mono_items })
    }
}
