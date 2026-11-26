//! Crate root.
/// M6.4 body queries (`thir_body` / `mir_build` / `def_map`) with early-cutoff
/// backdating, on `Database`.
pub mod body_queries;
/// Typed compilation phases (M5.1 Phase 1 + 2 + 3: trait extraction, core
/// pipeline extraction, driver final extraction).
pub mod phase;
pub mod phase_prof;
use glyim_codegen::CodegenBackend;
#[cfg(feature = "llvm-codegen")]
use glyim_codegen_llvm::LlvmBackend;
use glyim_core::def_id::TraitDefId as SolveTraitDefId;
use glyim_db::{Database, QueryContext};
use glyim_diag::{CompResult, DiagSink, GlyimDiagnostic};
use glyim_mir::Body;
use glyim_solve::solver::{BuiltinTrait, TraitDef};
pub use phase_prof::{Phase, PhaseProfile, PhaseRecorder, StatsReport};
use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

/// Register the compiler-known builtin traits (`Future`, `Drop`, `Deref`,
/// `Clone`, `Copy`, `Sized`, `Send`, `Sync`, `Unpin`, `Fn`/`FnMut`/`FnOnce`,
/// `Iterator`, `IntoIterator`, `Default`, `Any`) in the `TraitContext` so the
/// solver can resolve `impl Future for X` / `T: Clone` / `T: Send` bounds for
/// the standard library.
///
/// The table itself lives in
/// [`glyim_type::builtin_traits::BuiltinTraitDef`](glyim_type::builtin_traits::BuiltinTraitDef)
/// — the single source of truth shared with the resolver's
/// `TyCtxMut::register_builtin_ranges` — and is driven into the solver
/// through [`BuiltinTraitSink`](glyim_type::builtin_traits::BuiltinTraitSink).
/// This used to be a hand-copied table, and the copy had silently drifted:
/// `Default` (2016) and `Any` (2017) were registered in the type context but
/// never reached the solver, so solver-facing rendering saw an unknown trait
/// id for them. With the registry, a dropped entry fails the parity tests
/// (`builtin_trait_tests.rs`) instead of diverging silently.
///
/// The five auto/structural traits (`Copy`/`Sized`/`Send`/`Sync`/`Unpin`)
/// are additionally registered as lang traits via
/// [`structural_builtin_kind`] so `prove_trait` can discharge their bounds
/// structurally.
///
/// Public so out-of-tree harnesses (the test harness, LSP consumers, tooling)
/// can bootstrap a `TraitContext` that is guaranteed to agree with the
/// resolver's builtin table.
pub fn register_builtin_traits(
    trait_ctx: &mut glyim_solve::TraitContext,
    interner: &glyim_core::interner::Interner,
) {
    struct SolverBootstrapSink<'a> {
        trait_ctx: &'a mut glyim_solve::TraitContext,
        interner: &'a glyim_core::interner::Interner,
    }
    impl glyim_type::builtin_traits::BuiltinTraitSink for SolverBootstrapSink<'_> {
        fn register_builtin_trait(&mut self, trait_id: u32, name: &'static str) {
            let n = self.interner.intern(name);
            let tid = SolveTraitDefId::from_raw(trait_id);
            self.trait_ctx.register_trait(TraitDef {
                def_id: tid,
                name: n,
                associated_types: Vec::new(),
                predicates: Vec::new(),
            });
        }
    }

    glyim_type::builtin_traits::BuiltinTraitDef::register_all(&mut SolverBootstrapSink {
        trait_ctx,
        interner,
    });
    for t in glyim_type::builtin_traits::BuiltinTraitDef::ALL {
        if let Some(kind) = structural_builtin_kind(*t) {
            trait_ctx.register_lang_trait(SolveTraitDefId::from_raw(t.trait_id()), kind);
        }
    }
}

/// Exhaustive mapping from the shared registry's variants to the solver's
/// structural [`BuiltinTrait`] kinds. Every `BuiltinTraitDef` variant is
/// matched explicitly (no wildcard) so adding a variant to the registry
/// breaks this build until it is classified here as structural or not —
/// the compile-time half of the drift guarantee the registry's tests pin
/// at run time.
fn structural_builtin_kind(t: glyim_type::builtin_traits::BuiltinTraitDef) -> Option<BuiltinTrait> {
    use glyim_type::builtin_traits::BuiltinTraitDef as B;
    match t {
        B::Copy => Some(BuiltinTrait::Copy),
        B::Sized => Some(BuiltinTrait::Sized),
        B::Send => Some(BuiltinTrait::Send),
        B::Sync => Some(BuiltinTrait::Sync),
        B::Unpin => Some(BuiltinTrait::Unpin),
        B::Future => None,
        B::Drop => None,
        B::Deref => None,
        B::DerefMut => None,
        B::Clone => None,
        B::Fn => None,
        B::FnMut => None,
        B::FnOnce => None,
        B::Iterator => None,
        B::IntoIterator => None,
        B::Default => None,
        B::Any => None,
    }
}

/// External module loading (`mod foo;` -> `foo.g`).
pub mod mod_loader;
mod mono_cache;
mod pipeline_context;
use glyim_span::FileId;
use phase::CompilerPhase;

/// Thin builder chaining the extracted `CompilerPhase` structs (M5.1 Phase 3).
///
/// Holds the shared [`PhaseCtx`] and exposes one method per phase that
/// constructs the phase struct + dispatches through [`CompilerPhase::run`].
/// This absorbs the struct-literal line count — rustfmt's default
/// `struct_lit_width = 18` forces every phase struct whose `{ … }` content
/// exceeds 18 chars to multi-line, so constructing the phases inline in the
/// driver busts the ≤60-line `DoD`. Method calls obey `fn_call_width` (60),
/// not `struct_lit_width`, so each `chain.<phase>(…)` stays on one line.
/// The driver still chains phases **via the trait**: every method body is a
/// thin `Phase { … }.run(&mut self.pctx)` wrapper, so the dispatch is
/// `CompilerPhase::run`, not a hand-rolled call.
pub(crate) struct PipelineChain<'a> {
    pctx: phase::PhaseCtx<'a>,
}

impl<'a> PipelineChain<'a> {
    /// Construct the chain over the shared db/recorder/sink.
    pub(crate) fn new(
        db: &'a mut Database,
        recorder: &'a phase_prof::PhaseRecorder,
        sink: &'a RefCell<DiagSink>,
    ) -> Self {
        Self {
            pctx: phase::PhaseCtx { db, recorder, sink },
        }
    }

    /// Lex + parse (`LexParsePhase`).
    pub(crate) fn lex_parse(
        &mut self,
        path: &Path,
    ) -> CompResult<phase::lex_parse::LexParseOutput> {
        phase::lex_parse::LexParsePhase { path }.run(&mut self.pctx)
    }

    /// Expand + build def map (`DefMapPhase`).
    pub(crate) fn def_map(
        &mut self,
        root: &glyim_syntax::SyntaxNode,
        file_id: FileId,
        proc_registry: Option<&glyim_proc_macro::Registry>,
    ) -> CompResult<phase::def_map::DefMapOutput> {
        phase::def_map::DefMapPhase {
            root,
            file_id,
            proc_registry,
        }
        .run(&mut self.pctx)
    }

    /// Lower to HIR (`HirPhase`).
    pub(crate) fn hir(
        &mut self,
        expanded_root: &glyim_syntax::SyntaxNode,
        file_id: FileId,
    ) -> CompResult<phase::hir::HirOutput> {
        phase::hir::HirPhase {
            expanded_root,
            file_id,
        }
        .run(&mut self.pctx)
    }

    /// Type-check (`TypeckPhase`); publishes the frozen `TyCtx` to the db.
    pub(crate) fn typeck(
        &mut self,
        hir: &glyim_hir::CrateHir,
        def_map: &glyim_def_map::CrateDefMap,
    ) -> CompResult<phase::typeck::TypeckOutput> {
        phase::typeck::TypeckPhase { hir, def_map }.run(&mut self.pctx)
    }

    /// Lower THIR → MIR + borrowck + opt (`LowerPhase`).
    pub(crate) fn lower(
        &mut self,
        typed_bodies: &[(glyim_core::def_id::LocalDefId, glyim_typeck::TypedBody)],
        hir: &glyim_hir::CrateHir,
        const_values: &std::collections::HashMap<
            glyim_core::def_id::ConstDefId,
            glyim_const_eval::ConstValue,
        >,
        opt_level: u8,
        opt_bisect_limit: Option<usize>,
    ) -> CompResult<phase::lower::LowerOutput> {
        phase::lower::LowerPhase {
            typed_bodies,
            hir,
            const_values,
            opt_level,
            opt_bisect_limit,
        }
        .run(&mut self.pctx)
    }

    /// Monomorphize + post-mono checks (`MonoPhase`).
    pub(crate) fn mono(
        &mut self,
        parse_root: &glyim_syntax::SyntaxNode,
        hir: &glyim_hir::CrateHir,
        def_map: &glyim_def_map::CrateDefMap,
        mir_bodies_map: &std::collections::BTreeMap<
            glyim_core::def_id::DefId,
            std::sync::Arc<glyim_mir::Body>,
        >,
    ) -> CompResult<phase::mono::MonoOutput> {
        phase::mono::MonoPhase {
            parse_root,
            hir,
            def_map,
            mir_bodies_map,
        }
        .run(&mut self.pctx)
    }

    /// Post-mono emit-prep: annotation-needed + CGU + raw-generic filter (`EmitPhase`).
    pub(crate) fn emit(
        &mut self,
        lower: &phase::lower::LowerOutput,
        mono: &phase::mono::MonoOutput,
        unresolved_ty_var_spans: &std::collections::HashMap<
            glyim_core::def_id::LocalDefId,
            Vec<glyim_span::Span>,
        >,
        codegen_units: Option<usize>,
        emit_object: bool,
    ) -> CompResult<phase::emit::EmitOutput> {
        phase::emit::EmitPhase {
            lower,
            mono,
            unresolved_ty_var_spans,
            codegen_units,
            emit_object,
        }
        .run(&mut self.pctx)
    }
}

/// `Pipeline`.
pub struct Pipeline;

/// Artifacts produced by compiling a source file through the full pipeline.
/// Returned by [`Pipeline::compile_file_with_artifacts`] so that test harnesses
/// can inspect the mid-pipeline results (def-map, type-check, MIR bodies) that
/// the standard `compile_file` discards.
pub struct CompileArtifacts {
    /// `Struct`.
    pub def_map: glyim_def_map::CrateDefMap,
    /// `Struct`.
    pub typeck_result: glyim_typeck::TypeckResult,
    /// `Struct`.
    pub mir_bodies: Vec<(String, Arc<glyim_mir::Body>)>,
    /// `Struct`.
    pub ty_ctx: Arc<glyim_type::TyCtx>,
    /// Non-error diagnostics (warnings, notes) produced during a *successful*
    /// compile. The pipeline treats these as advisory — they never abort the
    /// compile — but the caller must render them: a user who never sees
    /// "unused generic parameter" or "large number of mono items" has no way
    /// to act on it. Before this field existed the CLI discarded every
    /// non-error diagnostic on success.
    pub warnings: Vec<GlyimDiagnostic>,
    /// Per-pass optimization stats (M5.11 `--opt-bisect-limit`): one
    /// [`glyim_opt::PassStats`] row per planned pass per optimized body,
    /// empty unless the compile ran bisected. `--time-passes` aggregates
    /// these into the per-pass table.
    pub pass_stats: Vec<(glyim_core::def_id::DefId, glyim_opt::PassStats)>,
    /// Per-phase wall-clock profile of the compile that produced these
    /// artifacts (`--time-passes` / `--self-profile` read this).
    pub profile: PhaseProfile,
}

/// Everything produced by the *front half* of the pipeline — parse → expand →
/// defmap → hir → typeck → lower → borrowck → opt → monomorphize — before any
/// code generation runs.
///
/// Every emit mode (`--emit=obj|exec|mir|llvm-ir|asm`) and the MIR-only
/// interpreter path must derive from a `PreparedCompilation` so they all see
/// the same *monomorphized* bodies. Previously `emit_llvm_ir` / `emit_asm`
/// re-implemented a partial chain that stopped before monomorphization, so
/// generic stdlib definitions reached codegen with `TyKind::Param` still in
pub(crate) struct PreparedCompilation {
    /// The crate's definition map.
    pub def_map: glyim_def_map::CrateDefMap,
    /// Type-check result (const values, THIR bodies, diagnostics).
    pub typeck_result: glyim_typeck::TypeckResult,
    /// Frozen type context (also published on the `Database`).
    pub ty_ctx: Arc<glyim_type::TyCtx>,
    /// Per-phase wall-clock profile of the compile that produced this
    /// value (the `--time-passes` / `--self-profile` / `--emit=stats`
    /// backbone). Recorded unconditionally; the cost is one `Instant`
    /// pair plus two `Cell` writes per phase.
    pub profile: PhaseProfile,
    /// Monomorphized + optimized MIR bodies, ready for codegen, each paired
    /// with its mangled LLVM symbol (`__glyim_fn_<id>[_<i>_<len>]`, see
    /// `glyim_type::mangle_fn_symbol`). Threading the symbol from mono
    /// through to codegen is what guarantees the *definition* the pipeline
    /// emits matches the *call-site* symbol codegen computes — so two
    /// instantiations of the same generic fn no longer collide on a plain
    /// `__glyim_fn_<id>`.
    pub all_bodies: Vec<(String, Arc<Body>)>,
    /// The same bodies as `all_bodies`, but **before** `glyim_opt::optimize`.
    /// Used by `--emit=mir` so the MIR dump reflects what the lowering pass
    /// produced rather than the post-DCE image — `fn main() { let mut i =
    /// 0; i += 1; let _ = i; }` is optimized to an empty body (the compound
    /// assign writes a local nothing reads), so a test looking for the
    /// `BinaryOp(Add, ...)` that `+=` lowers to needs the unoptimized form.
    /// Codegen modes still use `all_bodies`.
    pub pre_opt_bodies: Vec<(String, Arc<Body>)>,
    /// Per-pass optimization stats (M5.11 `--opt-bisect-limit`), one row per
    /// planned pass per optimized body, in the deterministic per-body loop
    /// order. Empty on the un-bisected path, where the optimizer collects no
    /// stats and the `--time-passes` output stays phase-granular.
    pub pass_stats: Vec<(glyim_core::def_id::DefId, glyim_opt::PassStats)>,
    /// Non-error diagnostics accumulated during the front half.
    pub warnings: Vec<GlyimDiagnostic>,
    // P2-A3 follow-up note: the raw-generic filter drops bodies before
    // emission; correlating a link-time `undefined symbol __glyim_fn_XXX`
    // with this filter (exposing the dropped symbols on the public
    // artifacts or warning per symbol) is a dedicated follow-up. The
    // interim `dropped_raw_generic` recording field was removed as dead
    // code (nothing could read it — `PreparedCompilation` is
    // `pub(crate)`), so re-add it *with a consumer* when the story is
    // wired.
}

/// The pipeline's `TyCtx` invariant: every post-analysis phase reads the
/// `TyCtx` the typeck phase installed into the database. A `None` here is
/// an ordering violation of the pipeline's own phase sequence — the public
/// entry points never produce it — so the failure is surfaced as an
/// internal-error diagnostic naming the phase which hit it (M5.10
/// panic-ratchet: no bare `panic!` on a missing `TyCtx`).
pub(crate) fn ty_ctx_after_analysis(
    db: &Database,
    phase: &'static str,
) -> CompResult<Arc<glyim_type::TyCtx>> {
    db.get_ty_ctx().ok_or_else(|| {
        vec![GlyimDiagnostic::internal_error(format!(
            "TyCtx not initialized: the {phase} phase ran before the typeck phase set it"
        ))]
    })
}

/// Load a crate's source into the VFS and **flatten its external modules**.
///
/// This is the single entry point every pipeline path (entry discovery,
/// full compile, MIR-only, `--emit=mir|llvm-ir|asm`) must use to obtain the
/// crate source. It reads `path` into the VFS, runs
/// [`mod_loader::flatten_modules`] to splice every bodyless `mod foo;` with
/// the contents of `foo.g` / `foo/mod.g`, then *writes the flattened source
/// back into the VFS* so every later `file_content(file_id)` read (codegen,
/// diagnostic rendering) sees the merged crate.
///
/// Centralising this is what keeps the emit paths consistent: previously
/// only `compile_file_with_artifacts` loaded external modules, so
/// `--emit=mir` silently produced error-typed MIR and `--emit=llvm-ir` `ICEd`
/// on the unresolved module path. Any new pipeline entry point must call this
/// rather than reading the VFS directly.
fn load_crate_source(
    db: &mut Database,
    path: &Path,
) -> Result<(FileId, Arc<str>), Vec<GlyimDiagnostic>> {
    // Prefer the VFS content already registered for `path` (the test harness
    // and `glyip` push in-memory source, which must NOT be re-read from
    // disk — the file may not exist there). Fall back to reading from disk
    // for the CLI path, which points at a real file.
    let file_id = match db.vfs().file_id(path) {
        Some(id) if db.file_text(id).is_some() => id,
        _ => db
            .vfs()
            .add_file_from_disk(path)
            .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("I/O Error: {e}"))])?,
    };
    let raw = db.file_text(file_id).unwrap_or_else(|| Arc::from(""));

    let (flattened, mod_diags) = mod_loader::flatten_modules(&raw, path, file_id);
    if mod_diags
        .iter()
        .any(|d| matches!(d.severity, glyim_diag::DiagSeverity::Error))
    {
        return Err(mod_diags);
    }
    let flattened: Arc<str> = Arc::from(flattened);
    // The id was minted (or resolved) above, so the write cannot fail by
    // construction; a refusal means the VFS invariant broke, and losing
    // the flattened source here would corrupt every later read.
    if let Err(err) = db.vfs().set_file_content(file_id, flattened.clone()) {
        return Err(vec![GlyimDiagnostic::internal_error(format!(
            "VFS write failed: {err}"
        ))]);
    }
    Ok((file_id, flattened))
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
        Self::entry_main_local_id_with_proc_registry(db, path, None)
    }

    /// Like [`entry_main_local_id`] but threads a proc-macro registry
    /// through the expansion pre-pass.
    ///
    /// **Why this exists.** The pre-pass has to reproduce the *exact*
    /// `LocalDefId` numbering the real pipeline will assign — that is how
    /// the id it returns identifies the same `main` the codegen will see.
    /// `include!` splices items and shifts ids, so the pre-pass runs the
    /// builtin expander. A proc macro that emits items also shifts ids; if
    /// the pre-pass skips it, the id it returns points at whatever item
    /// now sits in `main`'s old slot (verified: with
    /// `include!` doing the splicing the analogous bug ran the *included*
    /// function instead of `main`).
    ///
    /// Callers that do not have a registry (unit tests, small probes) keep
    /// using the wrapper above — no behaviour change for them.
    pub fn entry_main_local_id_with_proc_registry(
        db: &mut Database,
        path: &Path,
        proc_registry: Option<&glyim_proc_macro::Registry>,
    ) -> Option<u32> {
        let (file_id, source) = load_crate_source(db, path).ok()?;
        let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
        if parse_result
            .diagnostics
            .iter()
            .any(|d| matches!(d.severity, glyim_diag::DiagSeverity::Error))
        {
            return None;
        }
        // The *expanded* tree, not the raw parse. `expand_crate` can splice
        // items in (`include!`), rewrite a macro call into statements, or
        // (through the proc registry) generate whole items from an
        // attribute / function-like macro. Every inserted item shifts the
        // `LocalDefId`s the later def-map assigns — building this map from
        // the unexpanded tree therefore yields ids from a different
        // numbering (see the `include!` case that motivated this pre-pass
        // in the first place: the stale id for `main` pointed at an
        // included function).
        //
        // When `proc_registry` is supplied, proc expansion runs too, so the
        // numbering matches the real pipeline exactly. Without it, callers
        // get the same behaviour they had before this function existed.
        let mut hygiene = glyim_span::HygieneCtx::new();
        let mut expander = glyim_meta::Expander::new(&mut hygiene);
        expander.set_source_file(file_id);
        expander.set_vfs(db.vfs());
        if let Some(reg) = proc_registry {
            expander.with_proc_registry(Some(reg));
        }
        let (expanded_root, _) = expander.expand_crate(&parse_result.root);
        let (def_map, _def_diags) =
            glyim_def_map::build_def_map(&expanded_root, db.krate(), db.interner().clone());
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
        opt_level: u8,
    ) -> CompResult<()> {
        Self::compile_file_with_artifacts(
            db,
            path,
            backend,
            output_path,
            codegen_units,
            proc_registry,
            opt_level,
            None,
        )
        .map(|_| ())
    }

    /// Compute the content-addressed cache key for compiling `path` under the
    /// given build configuration, or `None` when the crate cannot be loaded
    /// (missing file, module cycle, …). The key incorporates the **flattened**
    /// crate source — every external module already spliced in — so a change
    /// to any `.g` file in the crate changes the key and invalidates the
    /// cached object.
    ///
    /// `has_entry_main` must be `true` when the caller will ask the backend
    /// for a C-ABI `main` wrapper (`--emit=exec`), since that changes the
    /// produced object.
    /// T128-PATCHED [CLI-4]: the cache key now also mixes in an `extra`
    /// byte slice. Callers (the CLI) pass the concatenated bytes of every
    /// `--proc-macro-deps` file. Without this, editing `pm.g` left the
    /// main source's key unchanged and the cache served an object
    /// produced by the old macro expansion.
    pub fn compute_cache_key(
        db: &mut Database,
        path: &Path,
        target: &str,
        opt_level: u8,
        has_entry_main: bool,
        extra: &[u8],
    ) -> Option<String> {
        let (_, source) = load_crate_source(db, path).ok()?;
        Some(glyim_db::cache::CompileCache::key_with_extra(
            &source,
            target,
            opt_level,
            has_entry_main,
            extra,
        ))
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
    /// Front half of the pipeline: parse → expand → defmap → hir → typeck →
    /// lower → borrowck → opt → monomorphize. Returns the monomorphized
    /// bodies plus the accumulated (non-error) diagnostics, *without* running
    /// codegen. Every emit mode and the MIR-only path derives from this so
    /// they all see the same monomorphized bodies.
    pub(crate) fn prepare_compilation(
        db: &mut Database,
        path: &Path,
        codegen_units: Option<usize>,
        proc_registry: Option<&glyim_proc_macro::Registry>,
        emit_object: bool,
        opt_level: u8,
        opt_bisect_limit: Option<usize>,
    ) -> CompResult<PreparedCompilation> {
        let recorder = phase_prof::PhaseRecorder::new();
        let sink = RefCell::new(DiagSink::new());
        let mut chain = PipelineChain::new(db, &recorder, &sink);
        let lp = chain.lex_parse(path)?;
        let dm = chain.def_map(&lp.parse_result.root, lp.file_id, proc_registry)?;
        let hir_out = chain.hir(&dm.expanded_root, lp.file_id)?;
        let tck = chain.typeck(&hir_out.hir, &dm.def_map)?;
        let lower = chain.lower(
            &tck.typeck_result.typed_bodies,
            &hir_out.hir,
            &tck.typeck_result.const_values,
            opt_level,
            opt_bisect_limit,
        )?;
        let mono = chain.mono(
            &lp.parse_result.root,
            &hir_out.hir,
            &dm.def_map,
            &lower.mir_bodies_map,
        )?;
        let emit = chain.emit(
            &lower,
            &mono,
            &tck.typeck_result.unresolved_ty_var_spans,
            codegen_units,
            emit_object,
        )?;
        // `chain` is not referenced after the last method call, so NLL ends its
        // lifetime here — releasing the `&mut Database` reborrow it holds before
        // `ty_ctx_after_analysis` reads the ty_ctx the typeck phase published.
        Ok(PreparedCompilation {
            def_map: dm.def_map,
            typeck_result: tck.typeck_result,
            ty_ctx: ty_ctx_after_analysis(db, "codegen")?,
            all_bodies: emit.all_bodies,
            pre_opt_bodies: emit.pre_opt_bodies,
            pass_stats: lower.pass_stats,
            warnings: sink.into_inner().into_diagnostics(),
            profile: recorder.profile(),
        })
    }

    /// Compile `path` to an object via `backend` and return the artifacts.
    ///
    /// Thin wrapper over [`Pipeline::prepare_compilation`]: run the front
    /// half, then hand the monomorphized bodies to the backend. This is the
    /// **only** entry point that runs codegen; every other emit mode formats
    /// a `PreparedCompilation` directly.
    pub fn compile_file_with_artifacts(
        db: &mut Database,
        path: &Path,
        backend: &dyn CodegenBackend,
        output_path: &Path,
        codegen_units: Option<usize>,
        proc_registry: Option<&glyim_proc_macro::Registry>,
        opt_level: u8,
        opt_bisect_limit: Option<usize>,
    ) -> CompResult<CompileArtifacts> {
        let prepared = Self::prepare_compilation(
            db,
            path,
            codegen_units,
            proc_registry,
            true,
            opt_level,
            opt_bisect_limit,
        )?;
        let mut profile = prepared.profile;
        let out_path = if output_path.as_os_str().is_empty() {
            Path::new("output.o")
        } else {
            output_path
        };
        if !prepared.all_bodies.is_empty() {
            // M2.1: charge backend code generation to the `codegen` slot of
            // the same profile the front half filled in.
            let cg_recorder = phase_prof::PhaseRecorder::new();
            let generate_result = {
                let _guard = cg_recorder.enter(Phase::Codegen);
                backend.generate(&prepared.all_bodies, out_path)
            };
            generate_result?;
            profile =
                profile.with_micros(Phase::Codegen, cg_recorder.profile().micros(Phase::Codegen));
        }
        Ok(CompileArtifacts {
            def_map: prepared.def_map,
            typeck_result: prepared.typeck_result,
            mir_bodies: prepared.all_bodies,
            ty_ctx: prepared.ty_ctx,
            warnings: prepared.warnings,
            pass_stats: prepared.pass_stats,
            profile,
        })
    }
}

/// Result of compiling a source file down to MIR without code generation.
pub struct MirCompilation {
    /// All monomorphized MIR bodies, keyed by their owning `DefId`.
    ///
    /// `BTreeMap` (not `HashMap`) so iteration is deterministic under the
    /// constitution C6 "no `HashMap` iteration in compiler core" rule
    /// (CT-9): every consumer that walks the map (`--emit=run`, `glyip`,
    /// the M4.7 metamorphic runner) sees the same DefId-ascending order
    /// across runs and machines.
    pub bodies: std::collections::BTreeMap<glyim_core::def_id::DefId, Arc<Body>>,
    /// The crate's definition map (used to resolve a function name to a `DefId`).
    pub def_map: glyim_def_map::CrateDefMap,
    /// The type context produced during type-checking (required by the interpreter).
    pub ty_ctx: Arc<glyim_type::TyCtx>,
    /// Non-error diagnostics (warnings, notes) produced during a *successful*
    /// compile, mirroring [`CompileArtifacts::warnings`]. Callers must render
    /// them: an MIR-only runner that silently drops a warning gives its user
    /// no way to act on it.
    pub warnings: Vec<GlyimDiagnostic>,
    /// Per-pass optimization stats (M5.11 `--opt-bisect-limit`): one
    /// [`glyim_opt::PassStats`] row per planned pass per optimized body,
    /// empty unless the compile ran bisected. `--time-passes` aggregates
    /// these into the per-pass table in run mode, mirroring
    /// [`CompileArtifacts::pass_stats`].
    pub pass_stats: Vec<(glyim_core::def_id::DefId, glyim_opt::PassStats)>,
    /// Per-phase wall-clock profile of the compile that produced this value
    /// (`--time-passes` / `--self-profile` read this in run mode), mirroring
    /// [`CompileArtifacts::profile`]. Recorded unconditionally by the shared
    /// front half, so every entry point publishes the same observability
    /// side-channels.
    ///
    /// [`CompileArtifacts::warnings`]: CompileArtifacts::warnings
    /// [`CompileArtifacts::pass_stats`]: CompileArtifacts::pass_stats
    /// [`CompileArtifacts::profile`]: CompileArtifacts::profile
    pub profile: PhaseProfile,
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
    #[must_use]
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
///
/// Registry-less wrapper for [`compile_file_to_mir_with_proc_registry`], kept
/// for the same reason as [`Pipeline::entry_main_local_id`]: unit-test and
/// legacy callers that don't have a proc-macro registry to give. Runners that
/// do (the `glyim` CLI's `--emit=run`, `glyip` with `proc-macro-deps`) must
/// call the registry-threaded variant so item-emitting proc macros shift the
/// `LocalDefId` numbering identically in the entry pre-pass and the compile.
pub fn compile_file_to_mir(
    db: &mut Database,
    path: &Path,
    opt_level: u8,
) -> Result<MirCompilation, Vec<GlyimDiagnostic>> {
    compile_file_to_mir_with_proc_registry(db, path, None, opt_level, None)
}

/// Compile a single source file to MIR only, threading the proc-macro
/// registry through the shared front half.
///
/// P1-C9 (production-readiness plan §6) routes this through
/// `prepare_compilation` so every phase (borrowck, `glyim_opt::optimize`,
/// monomorphization, the E0282 annotation check) runs exactly once with the
/// same diagnostics — there is no second, weaker MIR-only language
/// definition. The registry is threaded for the same reason
/// [`Pipeline::entry_main_local_id_with_proc_registry`] exists: an
/// item-emitting proc macro shifts `LocalDefId` numbering, and the entry
/// pre-pass and the compile must agree on it or the wrong body runs.
///
/// `opt_bisect_limit` is the M5.11 `--opt-bisect-limit=N` wire-up: `Some(n)`
/// runs only the first `n` passes of the opt-level's
/// [`glyim_opt::PassPlan`] (via `optimize_with_bisect`) and fills
/// [`MirCompilation::pass_stats`] with one row per planned pass; `None`
/// keeps the plain un-bisected optimize path with no stats collected.
///
/// [`Pipeline::entry_main_local_id_with_proc_registry`]: Pipeline::entry_main_local_id_with_proc_registry
pub fn compile_file_to_mir_with_proc_registry(
    db: &mut Database,
    path: &Path,
    proc_registry: Option<&glyim_proc_macro::Registry>,
    opt_level: u8,
    opt_bisect_limit: Option<usize>,
) -> Result<MirCompilation, Vec<GlyimDiagnostic>> {
    let prepared = Pipeline::prepare_compilation(
        db,
        path,
        None,
        proc_registry,
        false,
        opt_level,
        opt_bisect_limit,
    )?;
    let mut bodies = std::collections::BTreeMap::new();
    for (_symbol, body) in prepared.all_bodies {
        bodies.insert(body.owner, body);
    }
    Ok(MirCompilation {
        bodies,
        def_map: prepared.def_map,
        ty_ctx: prepared.ty_ctx,
        warnings: prepared.warnings,
        pass_stats: prepared.pass_stats,
        profile: prepared.profile,
    })
}

/// `emit_mir`.
pub fn emit_mir(
    db: &mut Database,
    input: &Path,
    output: &Path,
    proc_registry: Option<&glyim_proc_macro::Registry>,
    opt_level: u8,
) -> Result<PhaseProfile, Vec<GlyimDiagnostic>> {
    // Route through the shared front half (`prepare_compilation`) so the
    // body goes through the same diagnostics, monomorphization, and
    // raw-generic filtering as every other emit mode. The previous
    // standalone implementation re-implemented the entire pipeline
    // inline, which meant:
    //
    //   * the E0282 type-annotation-needed check (added in
    //     `prepare_compilation`) was silently skipped — `--emit=mir` on
    //     `fn main() -> i32 { Result::Ok(5).unwrap() }` succeeded where
    //     `--emit=obj` / `--emit=exec` correctly emitted "type
    //     annotations needed";
    //   * `all_bodies` was the *pre*-mono `thir_bodies` lowered one by
    //     one, so generic stdlib templates appeared as raw `Param`
    //     locals in the MIR dump;
    //   * the `body_is_raw_generic` filter never ran.
    //
    // Every other emit mode already routes through
    // `prepare_compilation` for exactly this reason (see
    // `emit_llvm_ir`'s comment).
    let prepared =
        Pipeline::prepare_compilation(db, input, None, proc_registry, false, opt_level, None)?;

    // `all_bodies` carries `(symbol, body)` pairs; the MIR dump is keyed
    // by the body's owner `DefId`, and `format_body` prints the owner's
    // local id. Iterate the pairs so the output preserves the mono
    // ordering.
    let mut out = String::new();
    for (_symbol, body) in &prepared.pre_opt_bodies {
        let formatted = format_body(body, &prepared.ty_ctx).map_err(|_| {
            vec![GlyimDiagnostic::internal_error(
                "MIR dump format failed".to_string(),
            )]
        })?;
        out.push_str(&formatted);
        out.push_str("\n\n");
    }
    std::fs::write(output, out)
        .map_err(|e| vec![GlyimDiagnostic::internal_error(e.to_string())])?;

    Ok(prepared.profile)
}
/// Emit the parser's output syntax tree (`--emit=syntax`).
///
/// Parse-only by design: no `Database` and no proc-macro registry are
/// involved — the dump is the raw parse tree *before* macro expansion, so
/// callers without a session can call this free of pipeline state.
pub fn emit_syntax(input: &Path, output: &Path) -> Result<PhaseProfile, Vec<GlyimDiagnostic>> {
    // `--emit=syntax` is a parse-only dump: it does not run the shared front
    // half. Its profile therefore covers exactly the two phases it executes
    // (load + parse) so `--time-passes` stays meaningful in this mode.
    let recorder = phase_prof::PhaseRecorder::new();
    let source = {
        let _guard = recorder.enter(Phase::Load);
        std::fs::read_to_string(input).map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "emit=syntax: read: {e}"
            ))]
        })?
    };
    let file_id = glyim_span::FileId::from_usize(0);
    let parsed = {
        let _guard = recorder.enter(Phase::Parse);
        glyim_frontend::parse_to_syntax(&source, file_id)
    };
    let dump = format!("{:#?}", parsed.root);
    std::fs::write(output, dump).map_err(|e| {
        vec![GlyimDiagnostic::internal_error(format!(
            "emit=syntax: write: {e}"
        ))]
    })?;
    Ok(recorder.profile())
}

/// `emit_llvm_ir`.
#[cfg(feature = "llvm-codegen")]
pub fn emit_llvm_ir(
    db: &mut Database,
    input: &Path,
    output: &Path,
    proc_registry: Option<&glyim_proc_macro::Registry>,
    opt_level: u8,
) -> Result<PhaseProfile, Vec<GlyimDiagnostic>> {
    // Route through the shared front half (`prepare_compilation`) so the
    // bodies are monomorphized. The previous standalone implementation
    // stopped after `lower_body` — before `discover_mono_roots` /
    // `MonoCtx::collect` — so generic stdlib definitions reached codegen with
    // `TyKind::Param` in their locals and `fn_abi_of` ICEd with
    // `UnknownType`. Any new emit mode MUST go through `prepare_compilation`.
    let prepared =
        Pipeline::prepare_compilation(db, input, None, proc_registry, false, opt_level, None)?;

    // Emit *every* monomorphized body (previously only `mir_bodies[0]` was
    // emitted, silently dropping the rest of the crate from the IR).
    let backend = LlvmBackend::with_db(db).with_debug_info(false);
    let mut ir = String::new();
    for (symbol, body) in &prepared.all_bodies {
        let one = backend
            .emit_ir_to_string(&prepared.ty_ctx, body, Some(symbol.as_str()))
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "LLVM IR generation failed: {e:?}"
                ))]
            })?;
        ir.push_str(&one);
        ir.push('\n');
    }
    std::fs::write(output, ir).map_err(|e| vec![GlyimDiagnostic::internal_error(e.to_string())])?;
    Ok(prepared.profile)
}

/// Compile a single `.g` file to **assembly** (`.s`) and write it to `output`.
/// Mirrors [`emit_llvm_ir`] but lowers through the LLVM backend's
/// `emit_assembly` path (plan §18.3: `--emit=asm`).
#[cfg(feature = "llvm-codegen")]
pub fn emit_asm(
    db: &mut Database,
    input: &Path,
    output: &Path,
    proc_registry: Option<&glyim_proc_macro::Registry>,
    opt_level: u8,
) -> Result<PhaseProfile, Vec<GlyimDiagnostic>> {
    // Route through the shared front half (`prepare_compilation`) — see
    // `emit_llvm_ir` for why the standalone chain was wrong (it skipped
    // monomorphization, so generic bodies ICEd in `fn_abi_of`).
    let prepared =
        Pipeline::prepare_compilation(db, input, None, proc_registry, false, opt_level, None)?;
    let backend = LlvmBackend::with_db(db).with_debug_info(false);
    backend
        .emit_assembly(&prepared.all_bodies, output)
        .map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "LLVM assembly generation failed: {e:?}"
            ))]
        })?;
    Ok(prepared.profile)
}

/// Measure a compile without emitting code: run the shared front half
/// (parse → expand → defmap → hir → typeck → lower → borrowck → opt →
/// monomorphize) and return the per-phase profile plus the mono/warning
/// counters. This is the engine behind `--emit=stats`.
///
/// Like every analysis-only path, no backend runs — the profile's
/// `codegen` slot stays 0 (see [`Phase::Codegen`]).
pub fn compile_stats(
    db: &mut Database,
    path: &Path,
    proc_registry: Option<&glyim_proc_macro::Registry>,
    opt_level: u8,
) -> Result<StatsReport, Vec<GlyimDiagnostic>> {
    let prepared =
        Pipeline::prepare_compilation(db, path, None, proc_registry, false, opt_level, None)?;
    Ok(StatsReport {
        profile: prepared.profile,
        mono_bodies: prepared.all_bodies.len(),
        warnings: prepared.warnings.len(),
    })
}

fn format_body(body: &Body, ctx: &glyim_type::TyCtx) -> Result<String, std::fmt::Error> {
    use std::fmt::Write;
    let mut s = String::new();
    // M2(d) micro-debt: writing to a `String` is infallible, so the prior
    // `.unwrap()` calls were panic-equivalents on an impossible error. They
    // now propagate via `?`, so a future change to a fallible sink (a writer
    // that can fail) needs no further edit here.
    writeln!(s, "fn {}() {{", body.owner)?;
    for (idx, local) in body.locals.iter_enumerated() {
        writeln!(
            s,
            "  ${}: {}",
            idx.to_raw(),
            glyim_type::PrintTy::new(local.ty, ctx)
        )?;
    }
    for (idx, block) in body.basic_blocks.iter_enumerated() {
        writeln!(s, "bb{}:", idx.to_raw())?;
        for stmt in &block.statements {
            writeln!(s, "  {:?}", stmt.kind)?;
        }
        writeln!(s, "  {:?}", block.terminator.kind)?;
    }
    writeln!(s, "}}")?;
    Ok(s)
}

#[cfg(test)]
mod tests;

pub(crate) fn closure_body_has_unmonomorphized_ty(
    body: &glyim_mir::Body,
    ctx: &glyim_type::TyCtx,
) -> bool {
    body.locals.iter().any(|l| ty_has_unmono(l.ty, ctx))
}

/// its `return_ty` is a rigid, unsubstituted type parameter (`Param`),
/// higher-ranked `Bound`, or an unnormalized `Projection`. Those cannot be
/// monomorphized (no concrete `T` to substitute) and must not reach codegen:
/// `fn_abi_of` calls `layout_of`, which ICEs with `UnknownType(Param)`.
///
/// We deliberately do *not* test locals here. Error-typed locals (and, in a
/// few edge cases, `Bound`/`Opaque` locals) are legitimate in fully
/// monomorphized bodies; flagging on them dropped valid bodies such as
/// `core/unsafecell_new.g` and `behavior/vec_is_empty.g`. Checking the
/// `return_ty` alone is enough to identify the specific raw-body leak that
/// motivated this filter.
/// Drop `[X0000] Err expression in THIR during lowering` warnings emitted by
/// a body that will never be instantiated.
///
/// A *raw generic* body — one whose return type is still `Param`/`Bound`/
/// `Projection` — is lowered to MIR for reference but filtered out before
/// emission (`body_is_raw_generic`). Its `Err` nodes are expected: typeck
/// emits a placeholder `Err` receiver for calls like `T::method(..)` on a
/// generic parameter (`check_expr.rs`'s `DynamicCall` arm), and the trait
/// default method `Iterator::fold<B>` contains exactly such a call. Nothing
/// ever substitutes the body, so the placeholder is never resolved — and
/// nothing ever runs it, so a warning about it is pure noise. (A genuine
/// `Err` in a *reachable* body still warns.)
pub(crate) fn drop_raw_generic_err_warnings(
    diags: &mut Vec<glyim_diag::GlyimDiagnostic>,
    body: &glyim_mir::Body,
    ctx: &glyim_type::TyCtx,
) {
    if !body_is_raw_generic(body, ctx) {
        return;
    }
    diags.retain(|d| d.message != "Err expression in THIR during lowering");
}

pub(crate) fn body_is_raw_generic(body: &glyim_mir::Body, ctx: &glyim_type::TyCtx) -> bool {
    use glyim_type::TyKind;
    matches!(
        ctx.ty_kind(body.return_ty),
        TyKind::Param(_) | TyKind::Bound(_, _) | TyKind::Projection(_)
    )
}

fn ty_has_unmono(ty: glyim_type::Ty, ctx: &glyim_type::TyCtx) -> bool {
    use glyim_type::{GenericArg, TyKind};
    fn go(ty: glyim_type::Ty, ctx: &glyim_type::TyCtx, depth: u32) -> bool {
        if depth > 64 {
            return true;
        }
        match ctx.ty_kind(ty).clone() {
            TyKind::Error
            | TyKind::Param(_)
            | TyKind::Bound(_, _)
            | TyKind::Projection(_)
            | TyKind::Opaque(_, _) => true,
            TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => go(inner, ctx, depth + 1),
            TyKind::Slice(inner) | TyKind::Array(inner, _) => go(inner, ctx, depth + 1),
            TyKind::Tuple(substs) | TyKind::Adt(_, substs) | TyKind::Closure(_, substs) => {
                ctx.substitution_args(substs).iter().any(|a| match a {
                    GenericArg::Ty(t) => go(*t, ctx, depth + 1),
                    _ => false,
                })
            }
            _ => false,
        }
    }
    go(ty, ctx, 0)
}
