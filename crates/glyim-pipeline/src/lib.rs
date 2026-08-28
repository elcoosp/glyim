//! Crate root.
use glyim_codegen::CodegenBackend;
use glyim_codegen_llvm::LlvmBackend;
use glyim_db::Database;
use glyim_diag::{CompResult, DiagSink, GlyimDiagnostic};
use glyim_lower::mono::MonoCtx;
use glyim_lower::post_mono_checks::{
    check_large_mono_set, check_unsized_locals, check_unused_generic_params,
};
use glyim_lower::partition::partition;
use glyim_mir::Body;
use glyim_solve::{InferenceTable, SimpleTraitSolver};
use glyim_solve::solver::ImplDef;
use glyim_type::TraitRef;
use glyim_typeck::tyconv;
use rayon::prelude::*;
use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

mod mono_cache;
mod pipeline_context;
use mono_cache::{
    PipelineMonoCache, compute_max_cgus, make_drop_glue_provider, make_mir_body_provider,
};
use pipeline_context::{PipelineBorrowckCtx, PipelineLowerCtx};

/// Pipeline.
pub struct Pipeline;

/// Artifacts produced by compiling a source file through the full pipeline.
/// Returned by [`Pipeline::compile_file_with_artifacts`] so that test harnesses
/// can inspect the mid-pipeline results (def-map, type-check, MIR bodies) that
/// the standard `compile_file` discards.
pub struct CompileArtifacts {
/// Struct.
    pub def_map: glyim_def_map::CrateDefMap,
/// Struct.
    pub typeck_result: glyim_typeck::TypeckResult,
/// Struct.
    pub mir_bodies: Vec<Arc<glyim_mir::Body>>,
/// Struct.
    pub ty_ctx: Arc<glyim_type::TyCtx>,
}

impl Pipeline {
    /// Resolve the crate's entry `main` function and return its `LocalDefId`
    /// raw index (or `None` if the program has no `fn main`). Used to tell the
    /// codegen backend which body must also receive a C-ABI `main` entry symbol
    /// so the produced object links into a runnable executable (`--emit=exec`).
    ///
    /// This is a lightweight pre-pass (parse + def-map only; no typeck/MIR) and
    /// exists because the backend is constructed *before* the full pipeline
    /// runs, so the entry `main` must be known up front.
    pub fn entry_main_local_id(db: &mut Database, path: &Path) -> Option<u32> {
        let file_id = db.vfs().add_file_from_disk(path).ok()?;
        let source = db.vfs().file_content(file_id).unwrap_or_else(|| Arc::from(""));
        let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
        if parse_result
            .diagnostics
            .iter()
            .any(|d| matches!(d.severity, glyim_diag::DiagSeverity::Error))
        {
            return None;
        }
        let (def_map, _def_diags) =
            glyim_def_map::build_def_map(&parse_result.root, db.krate(), db.interner().clone());
        let main_name = db.interner().intern("main");
        def_map
            .modules
            .get(def_map.root)
            .and_then(|m| m.resolve(main_name))
            .map(|(local_id, _vis): (glyim_core::def_id::LocalDefId, _)| local_id.to_raw())
    }

    /// Production entry point: compile a source file to an object file and
    /// discard the intermediate artifacts. Kept for `glyip` and other callers
    /// that only need the object file; test harnesses should use
    /// [`Pipeline::compile_file_with_artifacts`] to inspect the def-map /
    /// type-check / MIR results.
    pub fn compile_file(
        db: &mut Database,
        path: &Path,
        backend: &dyn CodegenBackend,
        output_path: &Path,
        codegen_units: Option<usize>,
        proc_registry: Option<&glyim_proc_macro::Registry>,
    ) -> CompResult<()> {
        Self::compile_file_with_artifacts(db, path, backend, output_path, codegen_units, proc_registry)
            .map(|_| ())
    }
}

impl Pipeline {
    /// Compile a source file through the full pipeline and return the mid-pipeline
    /// artifacts (def-map, type-check result, MIR bodies, type context) in addition
    /// to emitting the object file. Used by test harnesses that need to assert on
    /// the MIR/typeck output.
    ///
    /// `proc_registry`, when `Some` and non-empty, drives procedural-macro
    /// expansion (Phase 8 / plan §9.2): macro invocations in the source are
    /// expanded by the loaded proc-macro functions before HIR lowering. When
    /// `None` (or empty) the pipeline behaves exactly as before — no expansion
    /// pass runs, so existing non-proc-macro compiles are unaffected.
    pub fn compile_file_with_artifacts(
        db: &mut Database,
        path: &Path,
        backend: &dyn CodegenBackend,
        output_path: &Path,
        codegen_units: Option<usize>,
        proc_registry: Option<&glyim_proc_macro::Registry>,
    ) -> CompResult<CompileArtifacts> {
        let sink = DiagSink::new();
        let sink_cell = RefCell::new(sink);

        let file_id = db
            .vfs()
            .add_file_from_disk(path)
            .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("I/O Error: {}", e))])?;
        let source = db
            .vfs()
            .file_content(file_id)
            .unwrap_or_else(|| Arc::from(""));

        let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
        sink_cell
            .borrow_mut()
            .extend(parse_result.diagnostics.clone());
        if sink_cell.borrow().has_errors() {
            return Err(sink_cell.into_inner().into_diagnostics());
        }

        // Phase 8 / plan §9.2: expand proc-macro invocations (if a non-empty
        // registry was supplied) before def-map / HIR lowering. The expander
        // rewrites the syntax tree; if it returns diagnostics we surface them.
        // The `None`/`empty` case leaves `parse_result.root` untouched,
        // preserving prior behavior byte-for-byte.
        let expanded_root = if let Some(reg) = proc_registry {
            if !reg.is_empty() {
                let mut hygiene = glyim_span::HygieneCtx::new();
                let mut expander = glyim_meta::Expander::new(&mut hygiene);
                expander.with_proc_registry(Some(reg));
                let (expanded, expand_diags) = expander.expand_crate(&parse_result.root);
                sink_cell.borrow_mut().extend(expand_diags);
                if sink_cell.borrow().has_errors() {
                    return Err(sink_cell.into_inner().into_diagnostics());
                }
                expanded
            } else {
                parse_result.root.clone()
            }
        } else {
            parse_result.root.clone()
        };
        let (def_map, def_diagnostics) =
            glyim_def_map::build_def_map(&expanded_root, db.krate(), db.interner().clone());
        sink_cell.borrow_mut().extend(def_diagnostics);
        if sink_cell.borrow().has_errors() {
            return Err(sink_cell.into_inner().into_diagnostics());
        }

        let (hir, hir_diags) =
            glyim_hir::pipeline_api::lower_crate_for_pipeline(&expanded_root, db.intern_mut());
        sink_cell.borrow_mut().extend(hir_diags.clone());

        // Safety gate: a compiler must not emit a binary when HIR lowering
        // produced an Error-severity diagnostic. Notably this makes the async
        // desugar's `.await`-inside-a-loop guard (Error 60) fatal instead of a
        // silent miscompile — the lowered body would otherwise fall through to
        // the single-poll desugar whose `Pending` arm is an infinite `loop {}`
        // that hangs if ever executed. HIR errors are always real, so fail fast.
        if hir_diags
            .iter()
            .any(|d| matches!(d.severity, glyim_diag::DiagSeverity::Error))
        {
            return Err(sink_cell.into_inner().into_diagnostics());
        }

        // Plan unstub-5 P5: populate `glyim_solve::TraitContext` with the user's
        // trait impls so `SimpleTraitSolver::prove_trait` can actually find them
        // (previously `trait_ctx` was constructed empty, so every
        // `impl Trait for T` was invisible to the solver and any bound like
        // `F: Trait` resolved to `DefiniteNo`). This mirrors the impl
        // registration done for `TyCtx` inside `typeck_crate`, but targets the
        // solver's own `TraitContext`.
        let resolver = db.interner().clone();
        let mut ty_ctx_mut = glyim_type::TyCtxMut::new(resolver);
        let mut trait_ctx = glyim_solve::TraitContext::new();
        // Register every ADT's name -> id up front so the impl-header
        // resolution below (which needs each impl's `Self` type to resolve,
        // e.g. `impl Future for twoFuture` / `impl Future for depFuture`) can
        // find forward-referenced ADTs. The async state-machine desugar emits
        // a `FooState` enum whose variant field types reference *other*
        // generated ADTs, and those may be registered later in item order;
        // without this, their `impl Future` is invisible to the solver and
        // downstream `.poll()` calls report "Future not implemented".
        for (_id, item) in hir.items.iter_enumerated() {
            if matches!(
                &item.kind,
                glyim_hir::ItemKind::Enum(_) | glyim_hir::ItemKind::Struct(_)
            ) {
                // Allocate a stable id for this ADT: prefer a def-map-derived
                // id when the item came from source, otherwise mint a fresh
                // synthetic id (mirroring `tyconv::adt_id_for_item`). The
                // forward-reference correctness comes from registering the
                // name->id mapping NOW, before impl-header resolution below.
                let adt_id = def_map
                    .modules
                    .get(def_map.root)
                    .and_then(|m| m.scope.types.get(&item.name))
                    .map(|(id, _, _)| glyim_core::def_id::AdtId::from_raw(id.to_raw()))
                    .or_else(|| ty_ctx_mut.adt_id_by_name(item.name))
                    .unwrap_or_else(|| ty_ctx_mut.next_synthetic_adt_id());
                ty_ctx_mut.register_adt_with_name(
                    item.name,
                    adt_id,
                    glyim_type::AdtDef {
                        kind: match &item.kind {
                            glyim_hir::ItemKind::Enum(_) => glyim_type::AdtKind::Enum,
                            _ => glyim_type::AdtKind::Struct,
                        },
                        fields: glyim_core::arena::IndexVec::new(),
                        variants: Vec::new(),
                        generic_params: Vec::new(),
                    },
                );
            }
        }
        {
            let mut infer = InferenceTable::new();
            let mut impl_diags = Vec::new();
            for (_id, item) in hir.items.iter_enumerated() {
                if let glyim_hir::ItemKind::Impl(impl_item) = &item.kind {
                    let header = tyconv::resolve_impl_header(
                        &mut ty_ctx_mut,
                        &mut infer,
                        &def_map,
                        &mut impl_diags,
                        impl_item,
                        item.span,
                    );
                    if let Some(trait_def_id) = header.trait_def_id {
                        let impl_def_id = glyim_core::def_id::ImplDefId::from_raw(item.id.to_raw());
                        let substs = ty_ctx_mut
                            .intern_substitution(vec![glyim_type::GenericArg::Ty(header.self_ty)]);
                        trait_ctx.register_impl(ImplDef {
                            def_id: impl_def_id,
                            trait_ref: TraitRef {
                                def_id: trait_def_id,
                                substs,
                            },
                            predicates: Vec::new(),
                        });
                    }
                }
            }
        }
        let mut solver = SimpleTraitSolver::new(&trait_ctx);
        let (ty_ctx, typeck_result) =
            glyim_typeck::typeck_crate(ty_ctx_mut, &def_map, &hir, &mut solver);
        sink_cell.borrow_mut().extend(typeck_result.diagnostics.clone());
        if sink_cell.borrow().has_errors() {
            return Err(sink_cell.into_inner().into_diagnostics());
        }

        db.set_ty_ctx(ty_ctx);

        let mut closure_bodies_codegen: Vec<Arc<Body>> = Vec::new();
        let mir_bodies_map: std::collections::HashMap<glyim_core::def_id::DefId, Arc<Body>> = {
            let ty_ctx_guard = db.get_ty_ctx().expect("TyCtx not initialized");
            let ty_ctx_ref = ty_ctx_guard.as_ref();

            let lower_ctx = PipelineLowerCtx::new(ty_ctx_ref, &hir, typeck_result.const_values.clone());
            let mut bodies = std::collections::HashMap::new();

            for (_owner_def_id, thir_body) in &typeck_result.thir_bodies {
                let lower_result = glyim_lower::lower_body(&lower_ctx, thir_body);
                sink_cell.borrow_mut().extend(lower_result.diagnostics);
                if sink_cell.borrow().has_errors() {
                    return Err(sink_cell.into_inner().into_diagnostics());
                }
                let mir_body = lower_result.body;
                let owner = mir_body.owner;
                let mir_arc = Arc::new(mir_body);

                let borrowck_ctx = PipelineBorrowckCtx::new(ty_ctx_ref, &mir_arc);
                let borrowck_result = glyim_borrowck::check_borrows(&borrowck_ctx, &mir_arc);
                sink_cell.borrow_mut().extend(borrowck_result.errors);
                if sink_cell.borrow().has_errors() {
                    return Err(sink_cell.into_inner().into_diagnostics());
                }

                let opt_body = glyim_opt::optimize(ty_ctx_ref, &mir_arc);
                bodies.insert(owner, Arc::new(opt_body.body));

                // P6.2: closures captured while lowering this function are real
                // MIR bodies that must also be optimized, registered, and emitted
                // as `__glyim_fn_{closure_id}` by codegen.
                for (_cid, _substs, cbody) in lower_result.closure_bodies {
                    let c_mir_arc = Arc::new(cbody);
                    let c_borrowck_ctx =
                        PipelineBorrowckCtx::new(ty_ctx_ref, &c_mir_arc);
                    let c_borrowck_result =
                        glyim_borrowck::check_borrows(&c_borrowck_ctx, &c_mir_arc);
                    sink_cell.borrow_mut().extend(c_borrowck_result.errors);
                    if sink_cell.borrow().has_errors() {
                        return Err(sink_cell.into_inner().into_diagnostics());
                    }
                    let c_opt = glyim_opt::optimize(ty_ctx_ref, &c_mir_arc);
                    closure_bodies_codegen.push(Arc::new(c_opt.body));
                }
            }
            bodies
        };

        let (mono_roots, discovery_diags) = {
            let mut ty_ctx_mut_for_discovery = glyim_type::TyCtxMut::new(db.interner().clone());
            glyim_lower::discovery::discover_mono_roots(
                &parse_result.root,
                &hir,
                Some(&def_map),
                &mut ty_ctx_mut_for_discovery,
            )
        };
        sink_cell.borrow_mut().extend(discovery_diags);

        let mono_items: Vec<glyim_lower::mono::MonoItemData> = {
            let mut mono_ctx = MonoCtx::new();
            let ty_ctx_guard = db.get_ty_ctx().expect("TyCtx not initialized");
            let ty_ctx_ref = ty_ctx_guard.as_ref();
            let body_provider = make_mir_body_provider(&mir_bodies_map, &sink_cell, ty_ctx_ref);
            let drop_provider = make_drop_glue_provider(ty_ctx_ref);
            mono_ctx.with_ty_ctx(ty_ctx_ref);
            mono_ctx.collect(&mono_roots, &body_provider, &drop_provider);
            // §8.12: post-monomorphization semantic checks. These were dead
            // `#[allow(dead_code)]` functions; they now run here, immediately
            // after monomorphization, so unsized locals / unused generic params
            // / pathological mono-set sizes surface as diagnostics instead of
            // being silently ignored.
            let mut post_diags = check_unsized_locals(mono_ctx.items(), ty_ctx_ref);
            post_diags.extend(check_unused_generic_params(
                mono_ctx.items(),
                ty_ctx_ref,
            ));
            post_diags.extend(check_large_mono_set(mono_ctx.items(), 1000));
            sink_cell.borrow_mut().extend(post_diags);
            mono_ctx.items().to_vec()
        };

        if sink_cell.borrow().has_errors() {
            return Err(sink_cell.into_inner().into_diagnostics());
        }

        let cache = PipelineMonoCache::from_items(&mono_items);
        db.set_mono_cache(cache.symbols().to_vec());

        let max_cgus = codegen_units.unwrap_or_else(compute_max_cgus);
        let cgus = partition(&mono_items, max_cgus);

        let all_bodies: Vec<Arc<Body>> = if cgus.is_empty() {
            mir_bodies_map.into_values().collect()
        } else {
            let _cgu_stats: Vec<(usize, usize)> = cgus
                .par_iter()
                .map(|cgu_indices| {
                    let body_count = cgu_indices.len();
                    let total_locals: usize = cgu_indices
                        .iter()
                        .map(|&idx| mono_items[idx].body.locals.len())
                        .sum();
                    (body_count, total_locals)
                })
                .collect();

            cgus.iter()
                .flat_map(|cgu_indices| cgu_indices.iter().map(|&idx| mono_items[idx].body.clone()))
                .collect()
        };
        // Closure bodies are emitted after the functions that call them; LLVM
        // resolves the call via a forward-declared symbol, so order is fine.
        let mut all_bodies = all_bodies;
        all_bodies.extend(closure_bodies_codegen);

        let out_path = if output_path.as_os_str().is_empty() {
            Path::new("output.o")
        } else {
            output_path
        };

        if !all_bodies.is_empty() {
            backend.generate(&all_bodies, out_path)?;
        }

        let ty_ctx = db.get_ty_ctx().expect("TyCtx not initialized");
        Ok(CompileArtifacts {
            def_map,
            typeck_result: typeck_result.clone(),
            mir_bodies: all_bodies,
            ty_ctx,
        })
    }
}

/// Result of compiling a source file down to MIR without code generation.
pub struct MirCompilation {
    /// All monomorphized MIR bodies, keyed by their owning `DefId`.
    pub bodies: std::collections::HashMap<glyim_core::def_id::DefId, Arc<Body>>,
    /// The crate's definition map (used to resolve a function name to a `DefId`).
    pub def_map: glyim_def_map::CrateDefMap,
    /// The type context produced during type-checking (required by the interpreter).
    pub ty_ctx: Arc<glyim_type::TyCtx>,
}

impl MirCompilation {
    /// Monomorphize the whole program starting from `entry` (a function's
    /// `FnDefId`), returning the concrete MIR bodies keyed by `DefId`.
    ///
    /// Generic functions (e.g. `block_on<F: Future>`) are instantiated with
    /// their concrete argument types via [`glyim_lower::mono::MonoCtx`] and the
    /// substitution machinery in `mono_cache`, so a call like
    /// `block_on::<TwoFuture>(f)` yields a `block_on` body whose receiver `f`
    /// has the concrete type `TwoFuture`. Trait-method calls (`f.poll()`) are
    /// devirtualized against that concrete type during this pass, turning the
    /// `VirtualMethod` callee into a static `Fn` reference.
    ///
    /// The returned bodies are suitable for the in-process
    /// `glyim_mir_interp` interpreter: add each `(owner, body)` pair to the
    /// interpreter's function table and run the entry body.
    pub fn monomorphize(
        &self,
        entry: glyim_core::def_id::FnDefId,
    ) -> Vec<(glyim_core::def_id::DefId, Arc<glyim_mir::Body>)> {
        use glyim_lower::mono::{MonoCtx, MonoItem};

        let mut mono = MonoCtx::new();
        mono.with_ty_ctx(&self.ty_ctx);
        let sink = std::cell::RefCell::new(DiagSink::new());
        let mir_bodies =
            crate::mono_cache::make_mir_body_provider(&self.bodies, &sink, &self.ty_ctx);
        let drop_glue = |_ty: glyim_type::Ty| {
            Arc::new(glyim_mir::Body::dummy(glyim_core::def_id::DefId::new(
                glyim_core::def_id::CrateId::from_raw(0),
                glyim_core::def_id::LocalDefId::from_raw(0),
            )))
        };
        mono.collect(
            &[MonoItem::Fn {
                def_id: entry,
                substs: glyim_type::Substitution::empty(),
            }],
            &mir_bodies,
            &drop_glue,
        );
        mono.items()
            .iter()
            .map(|it| (it.body.owner, it.body.clone()))
            .collect()
    }
}

/// Compile a single source file to MIR only (no backend code generation).
///
/// This is the entry point used by `glyip test` and other runners that execute
/// MIR bodies directly (e.g. via the in-process `glyim_mir_interp` interpreter)
/// instead of producing a native object file.
pub fn compile_file_to_mir(
    db: &mut Database,
    path: &Path,
) -> Result<MirCompilation, Vec<GlyimDiagnostic>> {
    let sink = DiagSink::new();
    let sink_cell = RefCell::new(sink);

    let file_id = db
        .vfs()
        .add_file_from_disk(path)
        .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("I/O Error: {}", e))])?;
    let source = db
        .vfs()
        .file_content(file_id)
        .unwrap_or_else(|| Arc::from(""));

    let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
    sink_cell
        .borrow_mut()
        .extend(parse_result.diagnostics.clone());
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let (def_map, def_diagnostics) = glyim_def_map::build_def_map(&parse_result.root, db.krate(), db.interner().clone());
    sink_cell.borrow_mut().extend(def_diagnostics);
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let (hir, hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, db.intern_mut());
    sink_cell.borrow_mut().extend(hir_diags.clone());
    // Safety gate: HIR-level Error diagnostics (e.g. the async desugar's
    // `.await`-inside-a-loop guard, Error 60) must abort compilation instead
    // of silently lowering a miscompiling state machine.
    if hir_diags
        .iter()
        .any(|d| matches!(d.severity, glyim_diag::DiagSeverity::Error))
    {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let resolver = db.interner().clone();
    let ty_ctx_mut = glyim_type::TyCtxMut::new(resolver);
    let trait_ctx = glyim_solve::TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (ty_ctx, typeck_result) =
        glyim_typeck::typeck_crate(ty_ctx_mut, &def_map, &hir, &mut solver);
    sink_cell.borrow_mut().extend(typeck_result.diagnostics);
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    db.set_ty_ctx(ty_ctx);

    let ty_ctx_guard = db.get_ty_ctx().expect("TyCtx not initialized");
    let ty_ctx_ref = ty_ctx_guard.as_ref();
    let lower_ctx = PipelineLowerCtx::new(ty_ctx_ref, &hir, typeck_result.const_values.clone());

    let mut bodies = std::collections::HashMap::new();
    for (_owner_def_id, thir_body) in &typeck_result.thir_bodies {
        let lower_result = glyim_lower::lower_body(&lower_ctx, thir_body);
        sink_cell.borrow_mut().extend(lower_result.diagnostics);
        if sink_cell.borrow().has_errors() {
            return Err(sink_cell.into_inner().into_diagnostics());
        }
        let mir_arc = Arc::new(lower_result.body);
        let owner = mir_arc.owner;
        bodies.insert(owner, mir_arc);
    }

    // Drop-glue elaboration: lower `Drop` terminators into conditional drop
    // flag handling and array-drop loops (de-stubbing plan §15.2). Runs on a
    // `TyCtxMut` derived from the frozen `TyCtx` via `to_mut()`, which SHARES
    // the canonical type arena — so any `[bool; N]` flag-array types it
    // synthesizes are valid in the consuming `TyCtx` (the P0 interner fix that
    // previously made this path OOB-panic). After elaboration we re-freeze and
    // publish the updated context.
    {
        let frozen: &glyim_type::TyCtx = &ty_ctx_guard;
        let mut elaborator = frozen.to_mut();
        for (_owner, body_arc) in bodies.iter_mut() {
            let mut body = (**body_arc).clone();
            glyim_opt::elaborate_drops(&mut elaborator, &mut body);
            *body_arc = Arc::new(body);
        }
        let new_ty_ctx = elaborator.freeze();
        db.set_ty_ctx(new_ty_ctx);
    }
    let ty_ctx_guard = db.get_ty_ctx().expect("TyCtx not initialized after elaboration");

    Ok(MirCompilation {
        bodies,
        def_map,
        ty_ctx: ty_ctx_guard,
    })
}

/// emit_mir.
pub fn emit_mir(
    db: &mut Database,
    input: &Path,
    output: &Path,
) -> Result<(), Vec<GlyimDiagnostic>> {
    let sink = DiagSink::new();
    let sink_cell = RefCell::new(sink);

    let file_id = db
        .vfs()
        .add_file_from_disk(input)
        .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("I/O Error: {}", e))])?;
    let source = db
        .vfs()
        .file_content(file_id)
        .unwrap_or_else(|| Arc::from(""));

    let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
    sink_cell
        .borrow_mut()
        .extend(parse_result.diagnostics.clone());
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let (def_map, def_diagnostics) = glyim_def_map::build_def_map(&parse_result.root, db.krate(), db.interner().clone());
    sink_cell.borrow_mut().extend(def_diagnostics);
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let (hir, hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, db.intern_mut());
    sink_cell.borrow_mut().extend(hir_diags.clone());
    // Safety gate: HIR-level Error diagnostics (e.g. the async desugar's
    // `.await`-inside-a-loop guard, Error 60) must abort compilation instead
    // of silently lowering a miscompiling state machine.
    if hir_diags
        .iter()
        .any(|d| matches!(d.severity, glyim_diag::DiagSeverity::Error))
    {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let resolver = db.interner().clone();
    let ty_ctx_mut = glyim_type::TyCtxMut::new(resolver);
    let trait_ctx = glyim_solve::TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (ty_ctx, typeck_result) =
        glyim_typeck::typeck_crate(ty_ctx_mut, &def_map, &hir, &mut solver);
    sink_cell.borrow_mut().extend(typeck_result.diagnostics);
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    db.set_ty_ctx(ty_ctx);

    let ty_ctx_guard = db.get_ty_ctx().expect("TyCtx not initialized");
    let ty_ctx_ref = ty_ctx_guard.as_ref();
    let lower_ctx = PipelineLowerCtx::new(ty_ctx_ref, &hir, typeck_result.const_values.clone());
    let mut mir_bodies = Vec::new();

    for (_owner_def_id, thir_body) in &typeck_result.thir_bodies {
        let lower_result = glyim_lower::lower_body(&lower_ctx, thir_body);
        sink_cell.borrow_mut().extend(lower_result.diagnostics);
        if sink_cell.borrow().has_errors() {
            return Err(sink_cell.into_inner().into_diagnostics());
        }
        mir_bodies.push(lower_result.body);
    }

    let mut out = String::new();
    for body in &mir_bodies {
        out.push_str(&format_body(body, ty_ctx_ref));
        out.push_str("\n\n");
    }
    std::fs::write(output, out)
        .map_err(|e| vec![GlyimDiagnostic::internal_error(e.to_string())])?;

    Ok(())
}

/// emit_llvm_ir.
pub fn emit_llvm_ir(
    db: &mut Database,
    input: &Path,
    output: &Path,
) -> Result<(), Vec<GlyimDiagnostic>> {
    let sink = DiagSink::new();
    let sink_cell = RefCell::new(sink);

    let file_id = db
        .vfs()
        .add_file_from_disk(input)
        .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("I/O Error: {}", e))])?;
    let source = db
        .vfs()
        .file_content(file_id)
        .unwrap_or_else(|| Arc::from(""));

    let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
    sink_cell
        .borrow_mut()
        .extend(parse_result.diagnostics.clone());
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let (def_map, def_diagnostics) = glyim_def_map::build_def_map(&parse_result.root, db.krate(), db.interner().clone());
    sink_cell.borrow_mut().extend(def_diagnostics);
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let (hir, hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, db.intern_mut());
    sink_cell.borrow_mut().extend(hir_diags.clone());
    // Safety gate: HIR-level Error diagnostics (e.g. the async desugar's
    // `.await`-inside-a-loop guard, Error 60) must abort compilation instead
    // of silently lowering a miscompiling state machine.
    if hir_diags
        .iter()
        .any(|d| matches!(d.severity, glyim_diag::DiagSeverity::Error))
    {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let resolver = db.interner().clone();
    let ty_ctx_mut = glyim_type::TyCtxMut::new(resolver);
    let trait_ctx = glyim_solve::TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (ty_ctx, typeck_result) =
        glyim_typeck::typeck_crate(ty_ctx_mut, &def_map, &hir, &mut solver);
    sink_cell.borrow_mut().extend(typeck_result.diagnostics);
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    db.set_ty_ctx(ty_ctx);

    let ty_ctx_guard = db.get_ty_ctx().expect("TyCtx not initialized");
    let ty_ctx_ref = ty_ctx_guard.as_ref();
    let lower_ctx = PipelineLowerCtx::new(ty_ctx_ref, &hir, typeck_result.const_values.clone());
    let mut mir_bodies = Vec::new();

    for (_owner_def_id, thir_body) in &typeck_result.thir_bodies {
        let lower_result = glyim_lower::lower_body(&lower_ctx, thir_body);
        sink_cell.borrow_mut().extend(lower_result.diagnostics);
        if sink_cell.borrow().has_errors() {
            return Err(sink_cell.into_inner().into_diagnostics());
        }
        mir_bodies.push(lower_result.body);
    }

    if mir_bodies.is_empty() {
        return Err(vec![GlyimDiagnostic::internal_error(
            "No MIR bodies generated",
        )]);
    }

    let backend = LlvmBackend::new().with_debug_info(false);
    let ir = backend
        .emit_ir_to_string(ty_ctx_ref, &mir_bodies[0])
        .map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "LLVM IR generation failed: {:?}",
                e
            ))]
        })?;
    std::fs::write(output, ir).map_err(|e| vec![GlyimDiagnostic::internal_error(e.to_string())])?;

    Ok(())
}

/// Compile a single `.g` file to **assembly** (`.s`) and write it to `output`.
/// Mirrors [`emit_llvm_ir`] but lowers through the LLVM backend's
/// `emit_assembly` path (plan §18.3: `--emit=asm`).
pub fn emit_asm(
    db: &mut Database,
    input: &Path,
    output: &Path,
) -> Result<(), Vec<GlyimDiagnostic>> {
    let sink = DiagSink::new();
    let sink_cell = RefCell::new(sink);

    let file_id = db
        .vfs()
        .add_file_from_disk(input)
        .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("I/O Error: {}", e))])?;
    let source = db
        .vfs()
        .file_content(file_id)
        .unwrap_or_else(|| Arc::from(""));

    let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
    sink_cell
        .borrow_mut()
        .extend(parse_result.diagnostics.clone());
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let (def_map, def_diagnostics) = glyim_def_map::build_def_map(&parse_result.root, db.krate(), db.interner().clone());
    sink_cell.borrow_mut().extend(def_diagnostics);
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let (hir, hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, db.intern_mut());
    sink_cell.borrow_mut().extend(hir_diags.clone());
    // Safety gate: HIR-level Error diagnostics (e.g. the async desugar's
    // `.await`-inside-a-loop guard, Error 60) must abort compilation instead
    // of silently lowering a miscompiling state machine.
    if hir_diags
        .iter()
        .any(|d| matches!(d.severity, glyim_diag::DiagSeverity::Error))
    {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    let resolver = db.interner().clone();
    let ty_ctx_mut = glyim_type::TyCtxMut::new(resolver);
    let trait_ctx = glyim_solve::TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (ty_ctx, typeck_result) =
        glyim_typeck::typeck_crate(ty_ctx_mut, &def_map, &hir, &mut solver);
    sink_cell.borrow_mut().extend(typeck_result.diagnostics);
    if sink_cell.borrow().has_errors() {
        return Err(sink_cell.into_inner().into_diagnostics());
    }

    db.set_ty_ctx(ty_ctx);

    let ty_ctx_guard = db.get_ty_ctx().expect("TyCtx not initialized");
    let ty_ctx_ref = ty_ctx_guard.as_ref();
    let lower_ctx = PipelineLowerCtx::new(ty_ctx_ref, &hir, typeck_result.const_values.clone());
    let mut mir_bodies = Vec::new();

    for (_owner_def_id, thir_body) in &typeck_result.thir_bodies {
        let lower_result = glyim_lower::lower_body(&lower_ctx, thir_body);
        sink_cell.borrow_mut().extend(lower_result.diagnostics);
        if sink_cell.borrow().has_errors() {
            return Err(sink_cell.into_inner().into_diagnostics());
        }
        mir_bodies.push(lower_result.body);
    }

    if mir_bodies.is_empty() {
        return Err(vec![GlyimDiagnostic::internal_error(
            "No MIR bodies generated",
        )]);
    }

    let backend = LlvmBackend::new().with_debug_info(false);
    let arc_bodies: Vec<Arc<Body>> = mir_bodies.into_iter().map(Arc::new).collect();
    backend
        .emit_assembly(&arc_bodies, output)
        .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("LLVM assembly generation failed: {:?}", e))])?;

    Ok(())
}

fn format_body(body: &Body, ctx: &glyim_type::TyCtx) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    writeln!(s, "fn {}() {{", body.owner).unwrap();
    for (idx, local) in body.locals.iter_enumerated() {
        writeln!(
            s,
            "  ${}: {}",
            idx.to_raw(),
            glyim_type::PrintTy::new(local.ty, ctx)
        )
        .unwrap();
    }
    for (idx, block) in body.basic_blocks.iter_enumerated() {
        writeln!(s, "bb{}:", idx.to_raw()).unwrap();
        for stmt in &block.statements {
            writeln!(s, "  {:?}", stmt.kind).unwrap();
        }
        writeln!(s, "  {:?}", block.terminator.kind).unwrap();
    }
    writeln!(s, "}}").unwrap();
    s
}

#[cfg(test)]
mod tests;
