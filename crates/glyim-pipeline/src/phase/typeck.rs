//! Type-check phase (M5.1 Phase 3): build the trait context, register every
//! ADT / type-alias / impl-header, run `typeck_crate`, and publish the frozen
//! `TyCtx` to the database — extracting `prepare_compilation`'s inline
//! typeck-setup + `typeck_crate` block (the largest remaining slice of the
//! monolith) into a [`CompilerPhase`].
//!
//! # What moves out
//!
//! The `TyCtxMut`/`TraitContext` construction, the per-lint level forwarding,
//! the builtin-trait registration, the ADT forward-reference name→id
//! registration (so an `impl Future for FooState` whose `FooState` enum is
//! registered later in item order is still visible to the solver), the
//! type-alias template resolution, the impl-header resolution +
//! `TraitContext::register_impl`, the `SimpleTraitSolver::new`, and the
//! `typeck_crate` call itself. The frozen `TyCtx` the analysis produces is
//! published to the `Database` via `set_ty_ctx` (the side effect every later
//! phase's `ty_ctx_after_analysis` reads), and the `TypeckResult` (THIR
//! bodies, const values, unresolved-type-var spans, diagnostics) is the
//! phase's typed output.
//!
//! # Parity note
//!
//! The `alias_diags` / `impl_diags` produced by `tyconv::resolve_type_ref` /
//! `resolve_impl_header` are **not** extended into the diagnostic sink — the
//! monolith dropped them, relying on `typeck_crate`'s own diagnostic stream to
//! surface any real error. The extracted phase preserves this exactly so the
//! diagnostic set is byte-identical (the `M1` `DoD`). Closing those silent paths
//! is `M5.9`'s job, not the driver-extraction refactor's.
//!
//! # Why no `Option`-filled-later state
//!
//! Every input the phase needs (`hir`, `def_map`) is a typed field set at
//! construction; every piece of mutable state (`ty_ctx_mut`, `trait_ctx`,
//! `solver`) is a local built fresh inside `run`. The `TyCtx` the successor
//! phases read comes from `db.get_ty_ctx()`, which `set_ty_ctx` populates
//! before `run` returns — so `ty_ctx_after_analysis` in `LowerPhase` /
//! `MonoPhase` / `EmitPhase` can never observe a missing `TyCtx` from this
//! phase's success path.
use crate::Phase;
use crate::phase::{CompilerPhase, PhaseCtx, extract_diagnostics};
use crate::register_builtin_traits;
use glyim_core::interner::Name;
use glyim_def_map::{CrateDefMap, ModuleId};
use glyim_diag::CompResult;
use glyim_hir::CrateHir;
use glyim_hir::ItemKind;
use glyim_solve::solver::ImplDef;
use glyim_solve::{InferenceTable, SimpleTraitSolver, TraitContext};
use glyim_type::ParamTy;
use glyim_type::TraitRef;
use glyim_type::Ty;
use glyim_type::TyCtxMut;
use glyim_type::TyKind;
use glyim_typeck::TypeckResult;
use glyim_typeck::tyconv;
use std::collections::HashMap;

/// Output of [`TypeckPhase`]: the type-check result (THIR bodies, const
/// values, unresolved-type-var spans, diagnostics). The frozen `TyCtx` is
/// published to the `Database` as a side effect (`db.set_ty_ctx`), so it is
/// not duplicated in the output — successor phases read it via
/// `ty_ctx_after_analysis`.
pub struct TypeckOutput {
    /// The type-check result (`LowerPhase` reads `thir_bodies` + `const_values`;
    /// `EmitPhase` reads `unresolved_ty_var_spans`).
    pub typeck_result: TypeckResult,
}

/// Build the trait context, register every ADT / type-alias / impl-header,
/// run `typeck_crate`, and publish the frozen `TyCtx` to the database.
/// Extracted verbatim from `prepare_compilation`'s inline typeck block.
pub struct TypeckPhase<'a> {
    /// The lowered HIR crate (`HirOutput::hir`).
    pub hir: &'a CrateHir,
    /// The crate definition map (`DefMapOutput::def_map`).
    pub def_map: &'a CrateDefMap,
}

impl CompilerPhase for TypeckPhase<'_> {
    type Output = TypeckOutput;
    fn name(&self) -> &'static str {
        "typeck"
    }
    // Verbatim parity-preserving extraction of the monolith's typeck block (splitting it into helpers would break the 1:1 structural correspondence the M1 byte-identical-diagnostics contract pins).
    fn run(&self, ctx: &mut PhaseCtx) -> CompResult<Self::Output> {
        // Plan unstub-5 P5: populate `glyim_solve::TraitContext` with the user's
        // trait impls so `SimpleTraitSolver::prove_trait` can actually find them
        // (previously `trait_ctx` was constructed empty, so every
        // `impl Trait for T` was invisible to the solver and any bound like
        // `F: Trait` resolved to `DefiniteNo`). This mirrors the impl
        // registration done for `TyCtx` inside `typeck_crate`, but targets the
        // solver's own `TraitContext`.
        let resolver = ctx.db.interner().clone();
        let mut ty_ctx_mut = TyCtxMut::new(resolver);
        // §3.5.30 framework: forward per-lint levels from the Database.
        ty_ctx_mut.set_lints(ctx.db.lints().clone());
        let mut trait_ctx = TraitContext::new();
        {
            // Impl-header/alias resolution feeds typeck; charge it to the
            // typeck slot (accumulates with the `typeck_crate` call below).
            let _guard = ctx.recorder.enter(Phase::Typeck);
            register_builtin_traits(&mut trait_ctx, ctx.db.interner());
        }
        // Register every ADT's name -> id up front so the impl-header
        // resolution below (which needs each impl's `Self` type to resolve,
        // e.g. `impl Future for twoFuture` / `impl Future for depFuture`) can
        // find forward-referenced ADTs. The async state-machine desugar emits
        // a `FooState` enum whose variant field types reference *other*
        // generated ADTs, and those may be registered later in item order;
        // without this, their `impl Future` is invisible to the solver and
        // downstream `.poll()` calls report "Future not implemented".
        for (_id, item) in self.hir.items.iter_enumerated() {
            if matches!(&item.kind, ItemKind::Enum(_) | ItemKind::Struct(_)) {
                // Allocate a stable id for this ADT: prefer a def-map-derived
                // id when the item came from source, otherwise mint a fresh
                // synthetic id (mirroring `tyconv::adt_id_for_item`). The
                // forward-reference correctness comes from registering the
                // name->id mapping NOW, before impl-header resolution below.
                // Search *every* module's type scope (not just root): the
                // assembled stdlib wraps each module in `pub mod X { .. }`, so
                // ADTs like `Error`/`Result`/`Path`/`File` live in their own
                // module's scope, not the crate root. A root-only lookup would
                // miss them and mint a spurious synthetic id (e.g.
                // `Adt2000002`), which then mismatches the real id the
                // type-checker assigned — the "Adt45 vs Adt2000002" errors.
                // Script 499: register the NAME ONLY, not a placeholder
                // empty AdtDef. `register_adt_with_name` inserts into both
                // `adt_defs` and `adt_by_name`; writing an EMPTY AdtDef here
                // shadowed the real definition that `typeck_crate` would
                // later register. The typeck pass's `register_adt` call
                // writes the real def, but the earlier empty one had
                // already contributed to `adt_generation` and to
                // interior-mutability invalidation — and in the assembled
                // stdlib the empty def for `Option` (etc.) survived in a
                // way that broke variant matching (`match self { Some, None }`
                // → 'non-exhaustive match: missing variants None' plus
                // 'Adt1010<T> vs T').
                //
                // The block's actual purpose — making forward-referenced
                // ADTs visible to impl-header resolution below — only needs
                // the name→id mapping, which is what we do here.
                let adt_id = (0..self.def_map.modules.len())
                    .find_map(|i| {
                        self.def_map.modules[ModuleId::from_raw(i as u32)]
                            .scope
                            .types
                            .get(&item.name)
                    })
                    .map(|(id, _, _)| glyim_core::def_id::AdtId::from_raw(id.to_raw()))
                    .or_else(|| ty_ctx_mut.adt_id_by_name(item.name))
                    .unwrap_or_else(|| ty_ctx_mut.next_synthetic_adt_id());
                ty_ctx_mut.register_adt_name(item.name, adt_id);
            }
        }
        // Register type aliases (e.g. `type Result<T> = Result<T, Error>`) so
        // that 1-argument usages like `Result<usize>` expand to
        // `Result<usize, Error>`. The RHS is resolved to a `Ty` template with
        // the alias formals left as `TyKind::Param`, stored for later
        // expansion by `resolve_name_to_adt_ty` (stdlib-completion).
        //
        // Parity note: `alias_diags` is computed but, like the monolith, NOT
        // extended into the sink — `typeck_crate` surfaces the real errors.
        {
            let mut alias_infer = InferenceTable::new();
            let mut alias_diags = Vec::new();
            for (_id, item) in self.hir.items.iter_enumerated() {
                if let ItemKind::TypeAlias(alias) = &item.kind
                    && let Some(rhs) = &alias.ty
                {
                    let mut param_map: HashMap<Name, Ty> = HashMap::new();
                    let mut params: Vec<(u32, Name)> = Vec::new();
                    for (i, gp) in alias.generic_params.iter().enumerate() {
                        let pname = gp.name;
                        let pty = ty_ctx_mut.mk_ty(TyKind::Param(ParamTy {
                            index: i as u32,
                            name: pname,
                        }));
                        param_map.insert(pname, pty);
                        params.push((i as u32, pname));
                    }
                    let template = tyconv::resolve_type_ref(
                        &mut ty_ctx_mut,
                        &mut alias_infer,
                        self.def_map,
                        &mut alias_diags,
                        rhs,
                        &param_map,
                        item.span,
                    );
                    ty_ctx_mut.register_type_alias(item.name, params, template);
                }
            }
        }
        // Impl-header resolution + TraitContext registration. `impl_diags` is
        // computed but, like the monolith, NOT extended into the sink.
        {
            let mut infer = InferenceTable::new();
            let mut impl_diags = Vec::new();
            for (_id, item) in self.hir.items.iter_enumerated() {
                if let ItemKind::Impl(impl_item) = &item.kind {
                    let header = tyconv::resolve_impl_header(
                        &mut ty_ctx_mut,
                        &mut infer,
                        self.def_map,
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
        let (ty_ctx, typeck_result) = {
            let _guard = ctx.recorder.enter(Phase::Typeck);
            glyim_typeck::typeck_crate(ty_ctx_mut, self.def_map, self.hir, &mut solver)
        };
        ctx.sink
            .borrow_mut()
            .extend(typeck_result.diagnostics.clone());
        if ctx.sink.borrow().has_errors() {
            return Err(extract_diagnostics(ctx));
        }
        // Publish the frozen TyCtx to the Database so every successor phase
        // (`LowerPhase` / `MonoPhase` / `EmitPhase`) reads it via
        // `ty_ctx_after_analysis`. The publish happens before `run` returns,
        // so a success here guarantees the successor's `ty_ctx_after_analysis`
        // is `Some` — no `Option`-filled-later anti-pattern.
        ctx.db.set_ty_ctx(ty_ctx);
        Ok(TypeckOutput { typeck_result })
    }
}
