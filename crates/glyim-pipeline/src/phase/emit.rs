//! Emit-prep phase (M5.1 Phase 3): the post-monomorphization tail — the
//! type-annotation-needed (E0282) emission, the CGU partition + symbol pairing,
//! the raw-generic body filter, and the `--emit=mir` pre-optimization body
//! assembly — extracted from `prepare_compilation`'s inline tail into a
//! [`CompilerPhase`].
//!
//! # Why this is its own phase
//!
//! The tail is a cross-phase aggregate: the annotation-needed check needs both
//! `mono_items` (to know which owners reached codegen) and `typeck_result.
//! unresolved_ty_var_spans`; the CGU partition + symbol pairing needs
//! `mono_items`, `mir_bodies_map`, `closure_bodies_codegen`, and
//! `pre_opt_bodies_map` from three different phases (`LowerPhase`,
//! `MonoPhase`). The mono.rs comment (written before this extraction) said the
//! tail "stays inline" because of those cross-phase aggregates; the M5.1 Phase
//! 3 ≤60-line driver constraint forces it behind its own trait boundary
//! instead. Every line is moved verbatim — the only adaptation is
//! `mir_bodies_map.into_values()` → `.values().cloned()` (the map is now
//! borrowed from `LowerOutput`, not owned by the driver; `Arc::clone` is a
//! refcount bump, so the bodies are pointer-identical and the MIR bytes are
//! unchanged).
//!
//! # What the phase produces
//!
//! `EmitOutput { all_bodies, pre_opt_bodies }` — the symbol-paired,
//! raw-generic-filtered, emit-ready bodies (`all_bodies` for codegen / the
//! object cache; `pre_opt_bodies` for `--emit=mir`, so the dump shows the
//! lowering artifacts DCE would otherwise erase). The driver folds these into
//! `PreparedCompilation` alongside the `def_map`, `typeck_result`, `ty_ctx`, and
//! warnings the earlier phases already produced.
use crate::Phase;
use crate::mono_cache::{
    PipelineMonoCache, compute_max_cgus, make_drop_glue_provider, make_mir_body_provider,
};
use crate::phase::lower::LowerOutput;
use crate::phase::mono::MonoOutput;
use crate::phase::{CompilerPhase, PhaseCtx, extract_diagnostics};
use crate::ty_ctx_after_analysis;
use glyim_core::def_id::{FnDefId, LocalDefId};
use glyim_diag::{CompResult, GlyimDiagnostic};
use glyim_lower::mono::{MonoCtx, MonoItem};
use glyim_lower::partition::partition;
use glyim_mir::Body;
use glyim_span::Span;
use glyim_type::Substitution;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Output of [`EmitPhase`]: the emit-ready body maps.
pub struct EmitOutput {
    /// Monomorphized + optimized + raw-generic-filtered MIR bodies, each
    /// paired with its mangled symbol, ready for codegen.
    pub all_bodies: Vec<(String, Arc<Body>)>,
    /// The same bodies as `all_bodies`, but **before** `glyim_opt::optimize`
    /// (`--emit=mir` reads this so the dump shows lowering artifacts).
    pub pre_opt_bodies: Vec<(String, Arc<Body>)>,
}

/// The post-mono emit-prep tail. Extracted verbatim from
/// `prepare_compilation`'s inline annotation-needed + CGU + raw-generic
/// filter + `pre_opt` assembly block.
///
/// Carries every cross-phase input the tail needs: the mono item set, the
/// lowered + pre-opt body maps + closure codegen pairs (from `LowerOutput`),
/// and the unresolved-type-var spans (from `TypeckOutput`). `codegen_units`
/// and `emit_object` are the caller's compile-config knobs (the CGU cap and
/// the P2-A4 object-emit gate).
pub struct EmitPhase<'a> {
    /// The lower phase's full output (`mir_bodies_map`, `pre_opt_bodies_map`,
    /// `closure_bodies_codegen`) — bundled so the driver chains one ref per
    /// predecessor phase rather than four separate field borrows.
    pub lower: &'a LowerOutput,
    /// The mono phase's full output (`mono_items`) — bundled with `lower`.
    pub mono: &'a MonoOutput,
    /// Per-owner unresolved-type-var spans (`TypeckOutput::typeck_result.unresolved_ty_var_spans`).
    pub unresolved_ty_var_spans: &'a HashMap<LocalDefId, Vec<Span>>,
    /// The caller's `--codegen-units` cap (or `None` for the default).
    pub codegen_units: Option<usize>,
    /// P2-A4: whether the caller will emit a relocatable object. Only the
    /// object-emit path synthesizes mono roots for a bodyless library crate;
    /// analysis-only paths keep the pre-P2-A4 fallback.
    pub emit_object: bool,
}

impl CompilerPhase for EmitPhase<'_> {
    type Output = EmitOutput;
    fn name(&self) -> &'static str {
        "emit"
    }
    // Verbatim parity-preserving extraction of the monolith's post-mono tail; splitting it would break the 1:1 structural correspondence the `M1` byte-identical-diagnostics `DoD` pins.
    fn run(&self, ctx: &mut PhaseCtx) -> CompResult<Self::Output> {
        // Type-annotation-needed (rustc E0282) diagnostic emission.
        // Typeck collected, per-owner, the spans whose `Ty` still carried an
        // unbound `Infer(Ty(_))` after obligation fulfillment. Emitting
        // those for *every* owner would flag unused stdlib template bodies
        // whose unbound generics never get instantiated (they are never
        // monomorphized); those are benign. We emit only for owners that
        // actually reached the mono set — the same set that would reach
        // codegen and ICE on the unbound var. Owners of an unmonomorphized
        // stdlib body are dropped here and stay silent.
        {
            // FnDefId and LocalDefId share a raw id space (see
            // `MonoCtx::collect`), so a simple raw comparison suffices.
            let reachable: HashSet<u32> = self
                .mono
                .mono_items
                .iter()
                .filter_map(|item| match &item.item {
                    MonoItem::Fn { def_id, .. } => Some(def_id.to_raw()),
                    _ => None,
                })
                .collect();
            let mut ann_diags: Vec<GlyimDiagnostic> = Vec::new();
            for (owner, spans) in self.unresolved_ty_var_spans {
                if !reachable.contains(&owner.to_raw()) {
                    continue;
                }
                for span in spans {
                    ann_diags.push(GlyimDiagnostic::type_error(
                        *span,
                        "type annotations needed",
                    ));
                }
            }
            if !ann_diags.is_empty() {
                ctx.sink.borrow_mut().extend(ann_diags);
                return Err(extract_diagnostics(ctx));
            }
        }

        let cache = PipelineMonoCache::from_items(&self.mono.mono_items);
        ctx.db.set_mono_cache(cache.symbols().to_vec());

        let max_cgus = self.codegen_units.unwrap_or_else(compute_max_cgus);
        let cgus = partition(&self.mono.mono_items, max_cgus);

        // Pair each body with its mangled symbol. The symbol must be the
        // *same* string a call site's `lower_call` / `MirConstKind::Fn`
        // arm computes for this `(FnDefId, Substitution)` — otherwise the
        // linker sees an undefined reference and the ABI mismatches. The
        // `MonoItemData.symbol` field already holds the mangled form (see
        // `MonoCtx::collect`), so the CGU path just reads it; the empty-CGU
        // fallback (no mono roots) uses the legacy plain form since those
        // bodies were never substituted.
        let mut all_bodies: Vec<(String, Arc<Body>)> = if cgus.is_empty() {
            // P2-A4: library crate (no entry points). The primary mono pass
            // above ran with an empty root set, so `mono_items` is empty and
            // `cgus` is empty. Emitting pre-substitution bodies under plain
            // `__glyim_fn_N` symbols leaves their generic call sites
            // (`__glyim_fn_N_i_j`) undefined — see KNOWN_GAPS §3.5.20.
            //
            // Fix: run mono a second time with a synthetic root covering
            // every body actually lowered (`mir_bodies_map` keys — valid
            // `DefId`s, not HIR item indices; the HIR-index path was
            // attempt 3's bug and produced a single dummy `LocalDefId(0)`
            // symbol). Mono's normal transitive discovery then collects
            // every generic instantiation a root body references, and
            // `MonoItemData.symbol` carries the exact mangled form lowering
            // emits for the call site.
            //
            // Gated on `emit_object` so analysis-only paths
            // (`compile_file_to_mir` etc.) are unaffected — monomorphizing
            // the full stdlib surface is not legal for those
            // (`stdlib_full_probe` regression, KNOWN_GAPS §3.5.20).
            let synthesized: Option<Vec<(String, Arc<Body>)>> = if self.emit_object {
                let _guard = ctx.recorder.enter(Phase::Mono);
                let ty_ctx_guard = ty_ctx_after_analysis(ctx.db, "monomorphization")?;
                let ty_ctx_ref = ty_ctx_guard.as_ref();
                let empty_subst = Substitution::empty();
                let mut synthetic_roots: Vec<MonoItem> = Vec::new();
                for def_id in self.lower.mir_bodies_map.keys() {
                    synthetic_roots.push(MonoItem::Fn {
                        def_id: FnDefId::from_raw(def_id.local_id.to_raw()),
                        substs: empty_subst,
                    });
                }
                if synthetic_roots.is_empty() {
                    None
                } else {
                    let mut synth_ctx = MonoCtx::new();
                    synth_ctx.with_ty_ctx(ty_ctx_ref);
                    let body_provider =
                        make_mir_body_provider(&self.lower.mir_bodies_map, ctx.sink, ty_ctx_ref);
                    let drop_provider = make_drop_glue_provider(ty_ctx_ref);
                    synth_ctx.with_pre_subst_bodies(&self.lower.mir_bodies_map);
                    synth_ctx.collect(&synthetic_roots, &body_provider, &drop_provider);
                    Some(
                        synth_ctx
                            .items()
                            .iter()
                            .map(|item| (item.symbol.clone(), item.body.clone()))
                            .collect(),
                    )
                }
            } else {
                None
            };
            // Adapted from the monolith: `mir_bodies_map.into_values()` →
            // `.values().cloned()` because the map is now borrowed from
            // `LowerOutput` (not owned by the driver). `Arc::clone` is a
            // refcount bump — the bodies are pointer-identical, so the MIR
            // bytes the parity test hashes are unchanged.
            match synthesized {
                Some(v) if !v.is_empty() => v,
                _ => self
                    .lower
                    .mir_bodies_map
                    .values()
                    .cloned()
                    .map(|body| {
                        let sym = format!("__glyim_fn_{}", body.owner.local_id.to_raw());
                        (sym, body)
                    })
                    .collect(),
            }
        } else {
            cgus.iter()
                .flat_map(|cgu_indices| {
                    cgu_indices.iter().map(|&idx| {
                        let item = &self.mono.mono_items[idx];
                        (item.symbol.clone(), item.body.clone())
                    })
                })
                .collect()
        };
        // Closure bodies are emitted after the functions that call them; LLVM
        // resolves the call via a forward-declared symbol, so order is fine.
        all_bodies.extend(self.lower.closure_bodies_codegen.iter().cloned());

        // Step A (HANDOFF_SESSION_13_FINAL.md §2): drop *raw generic* bodies
        // (return_ty is `Param`/`Bound`/`Projection`) before emission. These
        // leak through mono and would ICE `fn_abi_of` on `UnknownType(Param)`.
        // See `body_is_raw_generic` for why we only test the return type.
        //
        // P2-A3: a downstream link error (`undefined symbol
        // __glyim_fn_XXX`) can be correlated with this filter; the interim
        // `dropped_raw_generic` recording field was removed as dead code,
        // so the `GLYIM_TMP_FILT` env-gated `eprintln!` trace is the only
        // observation point until the follow-up lands with a consumer.
        let mut dropped = 0usize;
        {
            let ty_ctx_guard = ty_ctx_after_analysis(ctx.db, "raw-generic filtering")?;
            let ty_ctx_ref = ty_ctx_guard.as_ref();
            let mut kept: Vec<(String, Arc<Body>)> = Vec::new();
            for (sym, body) in std::mem::take(&mut all_bodies) {
                if crate::body_is_raw_generic(&body, ty_ctx_ref) {
                    dropped += 1;
                } else {
                    kept.push((sym, body));
                }
            }
            all_bodies = kept;
            if dropped > 0 && std::env::var_os("GLYIM_TMP_FILT").is_some() {
                eprintln!("[GLYIM_FILT] dropped {dropped} raw-generic bodies from all_bodies");
            }
        }

        // For `--emit=mir`: use the pre-opt body when we have one, else
        // the mono/opt body (generic fns have no pre-opt non-generic
        // template in `pre_opt_bodies_map`, so fall back to the codegen
        // version — still useful and consistent with codegen).
        let pre_opt_bodies: Vec<(String, Arc<Body>)> = all_bodies
            .iter()
            .map(|(sym, b)| {
                let pre = self
                    .lower
                    .pre_opt_bodies_map
                    .get(&b.owner)
                    .cloned()
                    .unwrap_or_else(|| b.clone());
                (sym.clone(), pre)
            })
            .collect();

        Ok(EmitOutput {
            all_bodies,
            pre_opt_bodies,
        })
    }
}
