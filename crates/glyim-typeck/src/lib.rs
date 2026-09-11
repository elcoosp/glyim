//! Type checker: HIR → THIR with full inference and trait solving.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::vec_init_then_push,
    clippy::assertions_on_constants,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::manual_c_str_literals,
    clippy::doc_lazy_continuation,
    clippy::empty_line_after_doc_comments,
    clippy::manual_strip,
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::clone_on_copy,
    clippy::mutable_key_type,
    clippy::only_used_in_recursion,
    clippy::let_unit_value,
    clippy::unnecessary_literal_unwrap,
    clippy::format_in_format_args,
    clippy::permissions_set_readonly_false,
    clippy::needless_lifetimes,
    clippy::collapsible_if
)]
// Plan §3.1 (ratchet, not a hard ban): type checking consumes untrusted user
// input, so `unwrap`/`expect`/`panic!` on source-derived values must become
// `Diagnostic`/Result. `#[warn]` (not `#[deny]`) surfaces the count under
// `cargo clippy` so the sweep is tracked and ratcheted per-PR without
// breaking the build.
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod check_body;
mod check_expr;
mod check_pat;
mod check_stmt;
mod coherence;
mod deref_impl;
mod env;
pub mod thir;
/// tyconv.
pub mod tyconv;
mod unify;

use std::collections::HashMap;

use glyim_core::arena::IndexVec;
use glyim_core::def_id::{AdtId, ConstDefId, CrateId, DefId, FnDefId, LocalDefId, TraitDefId};
use glyim_core::interner::Name;
use glyim_core::primitives::{Abi, Mutability, Safety};
use glyim_diag::GlyimDiagnostic;
use glyim_hir::{ItemId, ItemKind, ExprId};
use glyim_def_map::{CrateDefMap, ModuleId, Resolver};
use glyim_solve::{FulfillmentCtx, InferenceTable, Obligation, ObligationCause, TraitContext};
use glyim_span::Span;
use glyim_type::{
    AdtDef, AdtKind, FieldDef, GenericArg, ImplPolarity, MethodDef, Predicate,
    TraitDef, TraitPredicate, TraitRef, Ty, TyCtx, TyCtxMut, VariantDef, FnSig,
};

#[derive(Clone, Debug)]
/// TypeckResult.
pub struct TypeckResult {
#[doc = "field"]
    pub thir_bodies: Vec<(LocalDefId, thir::Body)>,
/// Struct.
    pub diagnostics: Vec<GlyimDiagnostic>,
    /// Evaluated values of constant definitions (Part C: const value
    /// materialization). Populated during `typeck_crate` by const-evaluating
    /// each `const` initializer; consumed by MIR lowering (via `LowerCtx::
    /// const_value`) to fold `ConstRef` into a concrete `MirConstKind`.
    pub const_values: HashMap<ConstDefId, glyim_const_eval::ConstValue>,
    /// Resolved type of each HIR expression, keyed by the owning function's
    /// `LocalDefId` and the expression's `ExprId`. Populated during
    /// `typeck_crate` from each `FnCtxt::expr_cache` (which records the type
    /// of every checked expression) after inference variables are fully
    /// resolved. Drives downstream consumers such as LSP completion
    /// filtering (Tier 6.4).
    pub expr_types: HashMap<LocalDefId, HashMap<ExprId, Ty>>,
}

#[derive(Clone, Debug)]
/// Adjustment.
pub struct Adjustment {
/// Struct.
    pub kind: AdjustKind,
/// Struct.
    pub target: Ty,
}

#[derive(Clone, Debug)]
/// AdjustKind.
pub enum AdjustKind {
/// Variant.
    Deref,
#[allow(missing_docs)]
    Borrow(Mutability),
/// Variant.
    NeverToAny,
}

/// Register a top-level `enum`/`struct` as an ADT (variants/fields + generic
/// params) so that type resolution, struct-literal typing, field access, and
/// pattern matching can find it. The HIR and the def-map use distinct
/// interners, so the local id is resolved through `def_map.interner` (plan
/// unstub-5 P5). Returns `true` if the item was an ADT and got registered.
fn adt_id_for_item(
    ctx: &mut TyCtxMut,
    def_map: &glyim_def_map::CrateDefMap,
    module_id: glyim_def_map::ModuleId,
    name: glyim_core::interner::Name,
) -> AdtId {
    // Prefer a def-map-derived id (stable across all resolution passes) when
    // the item was lowered from source. For *generated* items (e.g. the future
    // struct produced by `async fn` desugaring) that are not in the def map,
    // reuse any synthetic id already assigned to this name during an earlier
    // pass so the same type gets a single, consistent `AdtId` everywhere.
    // Without this, two `register_adt_item` passes would each mint a distinct
    // synthetic id for a generated ADT, and type resolution (which keys on the
    // `AdtId`) would never reconcile them — producing an infinite loop when
    // projecting associated types through that ADT.
    // Prefer the item's *own defining module* (`module_id`) first: a `use`
    // re-export (e.g. `mod fs` does `use io::{ErrorKind}`) creates a *second*
    // def-map entry with a different `AdtId` (35) than the definition in `mod
    // io` (63). A blind `find_map` over all modules would return the re-export's
    // id (whichever module is first in iteration order) and split one logical
    // type across two ids. Checking the defining module first yields the real
    // definition's stable id. Only if it is absent there (a cross-module
    // reference from a module that does not itself define or re-export the
    // type) do we search every module's scope, then `adt_id_by_name`, then mint
    // a synthetic id.
    if let Some(l) = def_map
        .modules
        .get(module_id)
        .and_then(|m| m.scope.types.get(&name))
        .map(|(id, _, _)| *id)
    {
        return AdtId::from_raw(l.to_raw());
    }
    if let Some(l) = (0..def_map.modules.len())
        .find_map(|i| {
            def_map.modules[glyim_def_map::ModuleId::from_raw(i as u32)]
                .scope
                .types
                .get(&name)
        })
        .map(|(id, _, _)| *id)
    {
        return AdtId::from_raw(l.to_raw());
    }
    if let Some(existing) = ctx.adt_id_by_name(name) {
        return existing;
    }
    ctx.next_synthetic_adt_id()
}

fn register_adt_item(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    item: &glyim_hir::Item,
    module_id: glyim_def_map::ModuleId,
) -> bool {
    match &item.kind {
        glyim_hir::ItemKind::Enum(enum_item) => {
            let adt_id = adt_id_for_item(ctx, def_map, module_id, item.name);
            {
                // The enum's own generic params (e.g. `T` in `Poll<T>`) must be
                // in scope when resolving variant field types, so `Ready(T)`
                // yields a `TyKind::Param` rather than an `unresolved type`.
                let param_map = tyconv::build_param_tys(ctx, &enum_item.generic_params);
                let mut variants = Vec::new();
                for variant in &enum_item.variants {
                    let mut fields = IndexVec::new();
                    for field in &variant.fields {
                        let field_ty = tyconv::resolve_type_ref(
                            ctx, infer, def_map, diagnostics, &field.ty, &param_map, field.span,
                        );
                        fields.push(FieldDef { name: field.name, ty: field_ty });
                    }
                    // Plan §5.1 / §5.2: carry the variant's declared syntax
                    // style so the LSP can synthesize an arity-correct match
                    // arm and the diagnostic can carry structured shape.
                    let style = match variant.kind {
                        glyim_core::primitives::StructKind::Record => glyim_type::adt_def::VariantStyle::Struct,
                        glyim_core::primitives::StructKind::Tuple => glyim_type::adt_def::VariantStyle::Tuple,
                        glyim_core::primitives::StructKind::Unit => glyim_type::adt_def::VariantStyle::Unit,
                    };
                    variants.push(VariantDef { name: variant.name, fields, style });
                }
                ctx.register_adt_with_name(
                    item.name,
                    adt_id,
                    AdtDef {
                        kind: AdtKind::Enum,
                        fields: IndexVec::new(),
                        variants,
                        generic_params: enum_item.generic_params.iter().map(|p| p.name).collect(),
                    },
                );
            }
            true
        }
        glyim_hir::ItemKind::Struct(struct_item) => {
            let adt_id = adt_id_for_item(ctx, def_map, module_id, item.name);
            {
                let param_map = tyconv::build_param_tys(ctx, &struct_item.generic_params);
                let mut fields = IndexVec::new();
                for field in &struct_item.fields {
                    let field_ty = tyconv::resolve_type_ref(
                        ctx, infer, def_map, diagnostics, &field.ty, &param_map, field.span,
                    );
                    fields.push(FieldDef { name: field.name, ty: field_ty });
                }
                ctx.register_adt_with_name(
                    item.name,
                    adt_id,
                    AdtDef {
                        kind: AdtKind::Struct,
                        fields,
                        variants: Vec::new(),
                        generic_params: struct_item.generic_params.iter().map(|p| p.name).collect(),
                    },
                );
            }
            true
        }
        _ => false,
    }
}

#[tracing::instrument(level = "info", skip(ctx, solver))]
/// typeck_crate.
pub fn typeck_crate(
    mut ctx: TyCtxMut,
    def_map: &glyim_def_map::CrateDefMap,
    hir: &glyim_hir::CrateHir,
    solver: &mut dyn glyim_solve::TraitSolver,
) -> (TyCtx, TypeckResult) {
    let mut diagnostics = Vec::new();
    let mut infer = InferenceTable::new();
    let mut all_obligations: Vec<Obligation> = Vec::new();
    let trait_ctx = TraitContext::new();
    let mut thir_bodies: Vec<(LocalDefId, thir::Body)> = Vec::new();
    let mut all_expr_types: HashMap<LocalDefId, HashMap<ExprId, Ty>> = HashMap::new();
    let mut all_const_values: HashMap<ConstDefId, glyim_const_eval::ConstValue> = HashMap::new();

    // Maps each impl-method `BodyId` (assigned by the HIR def-counter) to the
    // `LocalDefId` typeck allocates for that method's MIR body. The two
    // counters are independent (HIR lowers all items in source order; typeck
    // uses its own `alloc_local_def_id`), so resolving a trait method to its
    // concrete impl function must translate through this map to hit the MIR
    // body key actually stored during monomorphization.
    let mut body_owner_map: HashMap<glyim_hir::BodyId, LocalDefId> = HashMap::new();

    let local_krate = def_map.krate;

    let mut next_local_def_id: u32 = 0;
    let alloc_local_def_id = |counter: &mut u32, diags: &mut Vec<GlyimDiagnostic>| -> LocalDefId {
        let id = *counter;
        *counter += 1;
        if *counter == u32::MAX {
            diags.push(GlyimDiagnostic::type_error(
                Span::DUMMY,
                "exhausted LocalDefId space",
            ));
        }
        LocalDefId::from_raw(id)
    };

    // any trait/impl/function type resolution. The coherence pass and the
    // signature-resolution loops below resolve types that reference these ADTs
    // (`Poll<...>` in a trait method return type, `AddOne { .. }` in a body),
    // so the ADTs must already be in `TyCtxMut`; otherwise `Poll`/`AddOne`-
    // style types are reported unresolved. Registration is idempotent, so the
    // later `register_adt` calls in `check_fn_items_in_module` are harmless.
    // Pass 1: register every ADT's name -> id BEFORE resolving any field
    // types. The async state-machine desugar emits a `FooState` enum whose
    // variant field types reference *other* generated ADTs (e.g.
    // `S0 { fut0: depFuture }`), and those referenced ADTs may be registered
    // LATER in `hir.items` iteration order. Resolving field types eagerly
    // (the old single-pass behaviour) turns such forward references into
    // `<error>` types and cascades into "no method `poll`" diagnostics.
    // Allocating every ADT id up front (with a placeholder def) lets the
    // field-type resolution in pass 2 see all ADTs regardless of order.
    for (_item_id, item) in hir.items.iter_enumerated() {
        if matches!(
            &item.kind,
            glyim_hir::ItemKind::Enum(_) | glyim_hir::ItemKind::Struct(_)
        ) {
            let adt_id = adt_id_for_item(&mut ctx, def_map, def_map.root, item.name);
            ctx.register_adt_with_name(
                item.name,
                adt_id,
                glyim_type::adt_def::AdtDef {
                    kind: match &item.kind {
                        glyim_hir::ItemKind::Enum(_) => glyim_type::adt_def::AdtKind::Enum,
                        _ => glyim_type::adt_def::AdtKind::Struct,
                    },
                    fields: IndexVec::new(),
                    variants: Vec::new(),
                    generic_params: Vec::new(),
                },
            );
        }
    }
    // Pass 2: now that every ADT name -> id is known, register the full
    // definitions (resolving variant/field types, which may reference other
    // ADTs forward or backward).
    for (_item_id, item) in hir.items.iter_enumerated() {
        register_adt_item(
            &mut ctx,
            &mut infer,
            def_map,
            &mut diagnostics,
            item,
            def_map.root,
        );
    }

    // 1. Coherence pass
    let mut coherence = coherence::CoherenceChecker::new(def_map);

    for (_item_id, item) in hir.items.iter_enumerated() {
        if let ItemKind::Trait(trait_item) = &item.kind {
            // Register the trait definition on the type context so that
            // `resolve_path_to_trait_def_id` (and other trait-namespace
            // lookups) can confirm a path names a *trait* rather than an ADT
            // or module. Method `fn_def_id`s are not tracked at the trait
            // level here (dispatch resolves to the concrete impl method), so
            // they stay `None`.
            let trait_path = glyim_hir::Path {
                segments: vec![glyim_hir::PathSegment {
                    name: item.name,
                    generic_args: None,
                }],
                kind: glyim_core::path::PathKind::Plain,
            };
            if let Some(local) = tyconv::resolve_path_to_local_def_id(&ctx, def_map, &trait_path) {
                let trait_def_id = TraitDefId::from_raw(local.to_raw());
                let methods = trait_item
                    .methods
                    .iter()
                    .map(|m| MethodDef {
                        name: m.name,
                        sig: FnSig {
                            inputs: ctx.intern_substitution(vec![]),
                            output: Ty::UNIT,
                            c_variadic: false,
                            unsafety: Safety::Safe,
                            abi: Abi::Glyim,
                        },
                        fn_def_id: None,
                    })
                    .collect();
                ctx.register_trait_def(
                    trait_def_id,
                    TraitDef {
                        name: item.name,
                        methods,
                        associated_types: trait_item.associated_types.iter().map(|a| a.name).collect(),
                    },
                );
            }
        }
    }

    for (_item_id, item) in hir.items.iter_enumerated() {
        if let ItemKind::Impl(impl_item) = &item.kind {
            let span = item.span;
            let header = tyconv::resolve_impl_header(
                &mut ctx,
                &mut infer,
                def_map,
                &mut diagnostics,
                impl_item,
                span,
            );

            // Plan unstub-5 P5: register the impl's associated-type definitions
            // into the projection table so `Self::Output` / `Type::Output` can
            // later be resolved to their defining type.
            if let (Some(trait_def_id), self_ty) = (header.trait_def_id, header.self_ty) {
                let param_map = tyconv::build_param_tys(&mut ctx, &impl_item.generic_params);
                let mut assoc_types = Vec::new();
                for at in &impl_item.associated_types {
                    if let Some(default_ty) = at.default.as_ref() {
                        let ty = tyconv::resolve_type_ref(
                            &mut ctx,
                            &mut infer,
                            def_map,
                            &mut diagnostics,
                            default_ty,
                            &param_map,
                            span,
                        );
                        assoc_types.push((at.name, ty));
                    }
                }
                if !assoc_types.is_empty() {
                    ctx.register_impl_assoc_types(self_ty, trait_def_id, assoc_types);
                }
            }

            if let Err(mut cohesion_diags) =
                coherence.check_and_register(header, &mut ctx, &mut infer)
            {
                diagnostics.append(&mut cohesion_diags);
            }
        }
    }

    // Phase 5 (GLYIM_DESTUB_PLAN): populate the `Deref` impl registry from real
    // `impl Deref for …` items so autoderef (`TyCtx::deref_ty`, consulted by
    // `resolve_method_call`) can step through user `Deref` impls for ADT
    // receivers (Box/Rc/Vec/etc.). Must run before method resolution consumes
    // the registry in the body-checking pass below.
    deref_impl::populate_deref_registry(&mut ctx, hir, def_map, &mut infer, &mut diagnostics);

    // 1b. Plan unstub-5 P5: populate `ctx.param_bounds` (param name → bound
    // trait def ids) from every function's / impl's generic params and
    // where-clauses. This lets associated-type projection (`F::Output`) and
    // method dispatch on a generic receiver (`f.poll()`) locate the bound
    // trait without a full trait solver. Name-keyed and crate-wide; parameter
    // names are interned and unique within a function.
    {
        let register_bounds = |ctx: &mut TyCtxMut, params: &[glyim_hir::GenericParam],
                                   where_clauses: &[glyim_hir::where_clause::WhereClause]| {
            for gp in params {
                if let glyim_hir::GenericParamKind::Type { bounds, .. } = &gp.kind {
                    for bound in bounds {
                        if let glyim_hir::TypeRef::Path(p) = bound {
                            if let Some(name) = p.as_name() {
                                if let Some(local) = tyconv::resolve_path_to_local_def_id(ctx, def_map, p)
                                {
                                    let tid = TraitDefId::from_raw(local.to_raw());
                                    ctx.param_bounds
                                        .entry(gp.name)
                                        .or_default()
                                        .push((name, tid));
                                }
                            }
                        }
                    }
                }
            }
            for wc in where_clauses {
                let wc_ty_name = match &wc.ty {
                    glyim_hir::TypeRef::Path(p) => p.as_name(),
                    _ => None,
                };
                if let Some(pname) = wc_ty_name {
                    for bound in &wc.bounds {
                        let p = &bound.trait_path;
                        if let Some(local) = tyconv::resolve_path_to_local_def_id(ctx, def_map, p) {
                            let tid = TraitDefId::from_raw(local.to_raw());
                            ctx.param_bounds.entry(pname).or_default().push((pname, tid));
                        }
                    }
                }
            }
        };
        for (_item_id, item) in hir.items.iter_enumerated() {
            match &item.kind {
                ItemKind::Fn(f) => {
                    register_bounds(&mut ctx, &f.generic_params, &f.where_clauses);
                }
                ItemKind::Impl(impl_item) => {
                    register_bounds(&mut ctx, &impl_item.generic_params, &impl_item.where_clauses);
                }
                _ => {}
            }
        }
    }

    // 2. Body checking pass
    // Gather ItemIds that are children of a ModItem so the flat pass skips
    // them — they are handled by the recursive fn walker below, which tracks
    // the def-map module context for correct LocalDefId alignment.
    let mut child_set: std::collections::HashSet<ItemId> =
        std::collections::HashSet::new();
    for (_id, item) in hir.items.iter_enumerated() {
        if let ItemKind::Mod(m) = &item.kind {
            for c in &m.children {
                child_set.insert(*c);
            }
        }
    }

    // Pre-register the signatures of every top-level function under the
    // def-map's `LocalDefId` *before* the body-checking main loop below. The
    // main loop type-checks `impl` method bodies (e.g. the desugared `poll`
    // methods), and those bodies can reference top-level functions — in
    // particular a desugared `async fn` wrapper such as `ready` when another
    // async fn (`one_step`) awaits it. `check_fn_items_in_module` already
    // registers these signatures, but it runs *after* the main loop, so a
    // poll body that calls such a wrapper resolves it to an unregistered
    // value and cascades into spurious "enum-variant value paths are not yet
    // supported" / `<error>` errors. Registering first keeps value-namespace
    // resolution order-independent (a single-`async fn` payload happens to
    // work only because its poll body references no top-level function). The
    // registration here is idempotent with the later `check_fn_items_in_module`
    // pass.
    for (_item_id, item) in hir.items.iter_enumerated() {
        if child_set.contains(&_item_id) {
            continue;
        }
        if let ItemKind::Fn(f) = &item.kind {
            // CRITICAL: the `FnDefId` we register the signature under MUST
            // match the id `check_path` produces when a later call site
            // resolves this function by name. `check_path` walks the def-map
            // resolver (`resolve_path_to_local_def_id`); a direct
            // `scope.values.get(&item.name)` lookup can miss generated/async
            // items because `item.name` lives in the HIR interner while the
            // scope keys live in the def-map interner, and the `item.id`
            // fallback then mints a *different* `LocalDefId`. That mismatch
            // leaves the real fn-sig registered under the wrong id while the
            // call site reads an unrelated fn-sig (a classic "returns
            // `Poll<?ty>` instead of the future struct" miscompile). Use the
            // same resolver path so the two ids agree.
            let local_def_id = {
                let resolver = Resolver::new(
                    &def_map.modules,
                    def_map.root,
                    def_map.root,
                );
                let core_path = glyim_core::Path {
                    segments: vec![glyim_core::PathSegment {
                        name: item.name,
                        generic_args: None,
                    }],
                    kind: glyim_core::path::PathKind::Plain,
                };
                resolver
                    .resolve_path(&core_path)
                    .values
                    .map(|(id, _)| id)
                    .unwrap_or_else(|| LocalDefId::from_raw(item.id.to_raw()))
            };
            let sig = tyconv::resolve_fn_sig(
                &mut ctx,
                &mut infer,
                def_map,
                &mut diagnostics,
                &f.params,
                &f.return_ty,
                &f.generic_params,
                item.span,
                None,
            );
            let inputs = ctx.intern_substitution(
                sig.param_tys.iter().map(|t| GenericArg::Ty(*t)).collect(),
            );
            ctx.register_fn_sig(
                FnDefId::from_raw(local_def_id.to_raw()),
                FnSig {
                    inputs,
                    output: sig.return_ty,
                    c_variadic: false,
                    unsafety: Safety::Safe,
                    abi: abi_from_name(f.abi, &ctx),
                },
            );
        }
    }

    for (item_id, item) in hir.items.iter_enumerated() {
        if child_set.contains(&item_id) {
            continue;
        }
        let item_span = item.span;

        match &item.kind {
            ItemKind::Fn(_) => {}

            ItemKind::Impl(impl_item) => {
                let impl_span = item_span;
                let param_map = tyconv::build_param_tys(&mut ctx, &impl_item.generic_params);
                let self_ty_opt = Some(tyconv::resolve_type_ref(
                    &mut ctx,
                    &mut infer,
                    def_map,
                    &mut diagnostics,
                    &impl_item.self_ty,
                    &param_map,
                    impl_span,
                ));

                for method in &impl_item.methods {
                    let local_def_id = alloc_local_def_id(&mut next_local_def_id, &mut diagnostics);
                    let owner = DefId::new(local_krate, local_def_id);
                    if let Some(bid) = method.body {
                        body_owner_map.insert(bid, local_def_id);
                    }

                    // Concatenate the impl's generic params with the method's own
                    // generic params so that method-level generics like `U` in
                    // `fn map<U>(...) -> Option<U>` are visible to `resolve_fn_sig`
                    // (which builds the param-map via `build_param_tys`).
                    let combined_generics: Vec<glyim_hir::GenericParam> =
                        impl_item.generic_params.iter()
                            .chain(method.generic_params.iter())
                            .cloned()
                            .collect();
                    let sig = tyconv::resolve_fn_sig(
                        &mut ctx,
                        &mut infer,
                        def_map,
                        &mut diagnostics,
                        &method.params,
                        &method.return_ty,
                        &combined_generics,
                        impl_span,
                        self_ty_opt,
                    );

                    // Register the resolved signature for the LLVM codegen pass
                    // (see the Fn arm for rationale).
                    let inputs = ctx.intern_substitution(
                        sig.param_tys.iter().map(|t| GenericArg::Ty(*t)).collect(),
                    );
                    ctx.register_fn_sig(
                        FnDefId::from_raw(local_def_id.to_raw()),
                        FnSig {
                            inputs,
                            output: sig.return_ty,
                            c_variadic: false,
                            unsafety: Safety::Safe,
                            abi: Abi::Glyim,
                        },
                    );

                    // Populate the trait-method dispatch table so generic-bound
                    // calls (`f.poll()`) can be devirtualized at mono/interp
                    // time. Only `impl Trait for Adt` (concrete self) is
                    // registered; the self ADT id is needed as the dispatch key.
                    if let (Some(trait_path), Some(self_ty)) =
                        (&impl_item.trait_ref, self_ty_opt)
                    {
                        if let Some(trait_def_id) = tyconv::resolve_path_to_trait_def_id(
                            def_map,
                            &ctx,
                            trait_path,
                            impl_span,
                        ) {
                            if let glyim_type::TyKind::Adt(self_adt_id, _) = ctx.ty_kind(self_ty) {
                                ctx.register_impl_method(
                                    trait_def_id,
                                    *self_adt_id,
                                    method.name,
                                    FnDefId::from_raw(local_def_id.to_raw()),
                                );
                            }
                        }
                    }

                    process_where_clauses(
                        &mut ctx,
                        &mut infer,
                        def_map,
                        &mut diagnostics,
                        &mut all_obligations,
                        &impl_item.generic_params,
                        &impl_item.where_clauses,
                        impl_span,
                    );

                    let body_id = method.body.or_else(|| {
                        find_trait_default_body(hir, &impl_item.trait_ref, method.name)
                    });

                    if let Some(body_id) = body_id {
                        let params: Vec<(Name, Ty, Span)> = method
                            .params
                            .iter()
                            .zip(sig.param_tys.iter())
                            .map(|(p, ty)| (p.name, *ty, p.span))
                            .collect();
                        check_body(
                            &mut ctx,
                            &mut infer,
                            &mut diagnostics,
                            &mut all_obligations,
                            hir,
                            body_id,
                            owner,
                            sig.return_ty,
                            &params,
                            &mut thir_bodies,
                            local_def_id,
                            def_map,
                            &trait_ctx,
                            &body_owner_map,
                            &mut all_expr_types,
                            def_map.root,
                        );
                    } else {
                        diagnostics.push(GlyimDiagnostic::type_error(
                            impl_span,
                            format!(
                                "method `{}` has no implementation and no default",
                                ctx.name_str(method.name)
                            ),
                        ));
                    }
                }
            }

            _ => {}
        }
    }

    // Function definitions (top-level and nested in `mod`) are registered
    // under the def-map's LocalDefId so value-namespace path resolution lines
    // up with the id `check_path` derives from the def map.
    let top_level_ids: Vec<ItemId> = hir
        .items
        .iter_enumerated()
        .filter(|(id, _)| !child_set.contains(id))
        .map(|(id, _)| id)
        .collect();
    check_fn_items_in_module(
        &mut ctx,
        &mut infer,
        def_map,
        &mut diagnostics,
        &mut all_obligations,
        hir,
        local_krate,
        &trait_ctx,
        &mut all_expr_types,
        &mut thir_bodies,
        &mut all_const_values,
        &top_level_ids,
        def_map.root,
        &mut next_local_def_id,
        &body_owner_map,
    );

    // 3. Obligation fulfillment
    let frozen_ctx = ctx.freeze();

    let mut fulfill = FulfillmentCtx::new(&frozen_ctx, solver);
    fulfill.extend(all_obligations);

    if let Err(overflow) = fulfill.process_obligations(100_000) {
        diagnostics.push(GlyimDiagnostic::type_error(
            Span::DUMMY,
            format!("overflow evaluating obligation: {:?}", overflow.predicate),
        ));
    }

    diagnostics.extend(fulfill.into_diagnostics());

    // Resolve inference variables in the collected per-expression types so the
    // public `expr_ty` query returns concrete types, not `TyKind::Infer(..)`.
    let mut expr_types: HashMap<LocalDefId, HashMap<ExprId, Ty>> = HashMap::new();
    for (body_id, raw) in &all_expr_types {
        let mut resolved: HashMap<ExprId, Ty> = HashMap::new();
        for (eid, ty) in raw {
            let ty = infer
                .fully_resolve(&frozen_ctx, *ty)
                .unwrap_or(*ty);
            resolved.insert(*eid, ty);
        }
        expr_types.insert(*body_id, resolved);
    }

    let result = TypeckResult {
        thir_bodies,
        diagnostics,
        const_values: all_const_values.clone(),
        expr_types,
    };
    (frozen_ctx, result)
}

/// Recursively type-check function definitions, walking the HIR `ModItem`
/// tree in lockstep with the def-map module tree.
///
/// Each function is registered under the def-map's `LocalDefId` (looked up by
/// name within its enclosing module), so the `FnDefId` used here matches the
/// one `check_path` derives from the def map when resolving a value-namespace
/// path (`foo`, `mod::foo`). The flat body-check loop in `typeck_crate` skips
/// `ModItem` children and delegates them here.
#[allow(clippy::too_many_arguments)]
fn check_fn_items_in_module(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    all_obligations: &mut Vec<Obligation>,
    hir: &glyim_hir::CrateHir,
    local_krate: CrateId,
    trait_ctx: &TraitContext,
    all_expr_types: &mut HashMap<LocalDefId, HashMap<ExprId, Ty>>,
    thir_bodies: &mut Vec<(LocalDefId, thir::Body)>,
    all_const_values: &mut HashMap<ConstDefId, glyim_const_eval::ConstValue>,
    item_ids: &[ItemId],
    module_id: ModuleId,
    next_local_def_id: &mut u32,
    body_owner_map: &HashMap<glyim_hir::BodyId, LocalDefId>,
) {
    for item_id in item_ids {
        let item = match hir.items.get(*item_id) {
            Some(i) => i,
            None => continue,
        };
        let item_span = item.span;
        match &item.kind {
            ItemKind::Fn(f) => {
                // The def map is the source of truth for a function's
                // LocalDefId; resolve it by name within the current module.
                // Functions not present in the def-map scope (e.g. `main`,
                // which the def-map treats specially) fall back to the legacy
                // per-item counter so they still get checked.
                let local_def_id = match def_map.modules[module_id]
                    .scope
                    .values
                    .get(&item.name)
                {
                    Some((id, _, _)) => *id,
                    // Fall back to the HIR `item.id` when the def-map does not
                    // track this item (e.g. `main`). This must match the id
                    // `discover_mono_roots` assigns to entry points so the
                    // MIR body is found during monomorphization.
                    None => LocalDefId::from_raw(item.id.to_raw()),
                };
                let owner = DefId::new(local_krate, local_def_id);

                let sig = tyconv::resolve_fn_sig(
                    ctx,
                    infer,
                    def_map,
                    diagnostics,
                    &f.params,
                    &f.return_ty,
                    &f.generic_params,
                    item_span,
                    None,
                );

                let inputs = ctx.intern_substitution(
                    sig.param_tys.iter().map(|t| GenericArg::Ty(*t)).collect(),
                );
                ctx.register_fn_sig(
                    FnDefId::from_raw(local_def_id.to_raw()),
                    FnSig {
                        inputs,
                        output: sig.return_ty,
                        c_variadic: false,
                        unsafety: Safety::Safe,
                        abi: abi_from_name(f.abi, ctx),
                    },
                );

                process_where_clauses(
                    ctx,
                    infer,
                    def_map,
                    diagnostics,
                    all_obligations,
                    &f.generic_params,
                    &f.where_clauses,
                    item_span,
                );

                if let Some(body_id) = f.body {
                    let params: Vec<(Name, Ty, Span)> = f
                        .params
                        .iter()
                        .zip(sig.param_tys.iter())
                        .map(|(p, ty)| (p.name, *ty, p.span))
                        .collect();
                    check_body(
                        ctx,
                        infer,
                        diagnostics,
                        all_obligations,
                        hir,
                        body_id,
                        owner,
                        sig.return_ty,
                        &params,
                        thir_bodies,
                        local_def_id,
                        def_map,
                        trait_ctx,
                        body_owner_map,
                        all_expr_types,
                        module_id,
                    );
                }
            }
            ItemKind::Enum(enum_item) => {
                // Register the enum as an ADT so variant field types are
                // available for data-carrying variant constructors and pattern
                // matching. Resolve the enum's `LocalDefId` from the current
                // module's type namespace (works for both top-level and
                // module-nested enums).
                let enum_local = def_map
                    .modules
                    .get(module_id)
                    .and_then(|m| m.scope.types.get(&item.name))
                    .map(|(id, _, _)| *id);
                if let Some(enum_local) = enum_local {
                    let adt_id = AdtId::from_raw(enum_local.to_raw());
                    // The enum's own generic params (e.g. `T` in `Poll<T>`) must
                    // be in scope when resolving variant field types, so
                    // `Ready(T)` yields a `TyKind::Param` rather than an
                    // `unresolved type`. (Mirrors `register_adt_item`.)
                    let param_map = tyconv::build_param_tys(ctx, &enum_item.generic_params);
                    let mut variants = Vec::new();
                    for variant in &enum_item.variants {
                        let mut fields = IndexVec::new();
                        for field in &variant.fields {
                            let field_ty = tyconv::resolve_type_ref(
                                ctx,
                                infer,
                                def_map,
                                diagnostics,
                                &field.ty,
                                &param_map,
                                field.span,
                            );
                            fields.push(FieldDef {
                                name: field.name,
                                ty: field_ty,
                            });
                        }
                        variants.push(VariantDef {
                            name: variant.name,
                            style: match variant.kind {
                                glyim_core::primitives::StructKind::Record => glyim_type::adt_def::VariantStyle::Struct,
                                glyim_core::primitives::StructKind::Tuple => glyim_type::adt_def::VariantStyle::Tuple,
                                glyim_core::primitives::StructKind::Unit => glyim_type::adt_def::VariantStyle::Unit,
                            },
                            fields,
                        });
                    }
                    ctx.register_adt(
                        adt_id,
                        AdtDef {
                            kind: AdtKind::Enum,
                            fields: IndexVec::new(),
                            variants,
                            generic_params: enum_item.generic_params.iter().map(|p| p.name).collect(),
                        },
                    );
                }
            }
            ItemKind::Struct(struct_item) => {
                // Register the struct as an ADT so its field types are
                // available for struct-literal typing, field access, and
                // pattern destructuring. Resolve the struct's `LocalDefId`
                // from the current module's type namespace.
                let struct_local = def_map
                    .modules
                    .get(module_id)
                    .and_then(|m| m.scope.types.get(&item.name))
                    .map(|(id, _, _)| *id);
                if let Some(struct_local) = struct_local {
                    let adt_id = AdtId::from_raw(struct_local.to_raw());
                    // Generic params of the struct are in scope when resolving
                    // its field types (e.g. `struct S<T> { x: T }`). Build the
                    // param map so `T` resolves to a type parameter.
                    let param_map = tyconv::build_param_tys(ctx, &struct_item.generic_params);
                    let mut fields = IndexVec::new();
                    for field in &struct_item.fields {
                        let field_ty = tyconv::resolve_type_ref(
                            ctx,
                            infer,
                            def_map,
                            diagnostics,
                            &field.ty,
                            &param_map,
                            field.span,
                        );
                        fields.push(FieldDef {
                            name: field.name,
                            ty: field_ty,
                        });
                    }
                    ctx.register_adt(
                        adt_id,
                        AdtDef {
                            kind: AdtKind::Struct,
                            fields,
                            variants: Vec::new(),
                            generic_params: struct_item.generic_params.iter().map(|p| p.name).collect(),
                        },
                    );
                }
            }
            ItemKind::Mod(m) => {
                let child_mod = def_map.modules[module_id]
                    .children
                    .iter()
                    .find(|(n, _)| *n == item.name)
                    .map(|(_, id)| *id);
                if let Some(child_mod) = child_mod {
                    check_fn_items_in_module(
                        ctx,
                        infer,
                        def_map,
                        diagnostics,
                        all_obligations,
                        hir,
                        local_krate,
                        trait_ctx,
                        all_expr_types,
                        thir_bodies,
                        all_const_values,
                        &m.children,
                        child_mod,
                        next_local_def_id,
                        body_owner_map,
                    );
                }
            }
            ItemKind::Const(c) => {
                // Resolve the constant's value type and register it so
                // `check_path` can produce a `ConstRef` with the right type.
                // The constant's body is not yet evaluated/threaded to codegen
                // (const value materialization is a follow-up); only its type
                // is needed for path-resolution type checking.
                let const_def_id = match def_map.modules[module_id]
                    .scope
                    .values
                    .get(&item.name)
                {
                    Some((id, _, _)) => ConstDefId::from_raw(id.to_raw()),
                    None => {
                        let id = *next_local_def_id;
                        *next_local_def_id += 1;
                        ConstDefId::from_raw(id)
                    }
                };
                let empty_params: HashMap<Name, Ty> = HashMap::new();
                let const_ty = tyconv::resolve_type_ref(
                    ctx,
                    infer,
                    def_map,
                    diagnostics,
                    &c.ty,
                    &empty_params,
                    item_span,
                );
                ctx.register_const_ty(const_def_id, const_ty);

                // Part C: const value materialization. Evaluate the constant's
                // initializer body (lowered in HIR) to a `ConstValue` and store
                // it so MIR lowering can fold `ConstRef` into a concrete
                // `MirConstKind`. Evaluation failures are surfaced as
                // diagnostics rather than silently producing a wrong value.
                if let (Some(body_id), Some(root_expr)) = (c.body, c.root_expr) {
                    let primitive_tys = glyim_const_eval::ConstEvaluator::build_primitive_tys(ctx);
                    let body = &hir.bodies[body_id];
                    let mut evaluator = glyim_const_eval::ConstEvaluator::new(body)
                        .with_interner(ctx.resolver())
                        .with_ty_ctx(ctx, primitive_tys);
                    match evaluator.evaluate(root_expr) {
                        Ok(value) => {
                            all_const_values.insert(const_def_id, value);
                        }
                        Err(e) => {
                            diagnostics.push(e.into_diagnostic());
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_body(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    pending_obligations: &mut Vec<Obligation>,
    hir: &glyim_hir::CrateHir,
    body_id: glyim_hir::BodyId,
    owner: DefId,
    return_ty: Ty,
    params: &[(Name, Ty, Span)],
    thir_bodies: &mut Vec<(LocalDefId, thir::Body)>,
    local_def_id: LocalDefId,
    def_map: &glyim_def_map::CrateDefMap,
    trait_ctx: &TraitContext,
    body_owner_map: &HashMap<glyim_hir::BodyId, LocalDefId>,
    expr_types: &mut HashMap<LocalDefId, HashMap<ExprId, Ty>>,
    module_id: glyim_def_map::ModuleId,
) {
    let body = &hir.bodies[body_id];
    let env = env::LocalEnv::new();

    // Resolve the module that lexically contains this function (the declaring
    // module if registered, otherwise the explicitly-passed module, otherwise
    // the crate root). Path resolution starts here and walks up to the root.
    let current_module = module_id;

    let fn_ctxt = check_body::FnCtxt {
        ctx,
        infer,
        diagnostics,
        pending_obligations,
        hir,
        body,
        env,
        return_ty,
        owner,
        expr_cache: Default::default(),
        def_map,
        current_module,
        trait_ctx,
        capture_log: Vec::new(),
        body_owner_map,
    };

    let (thir_body, body_expr_types) = fn_ctxt.check(params);
    thir_bodies.push((local_def_id, thir_body));
    // Hoist the per-expression type cache out of the (now-dropped) FnCtxt so
    // typeck_crate can resolve inference variables once obligation
    // fulfillment is complete.
    expr_types.insert(local_def_id, body_expr_types);
}

#[allow(clippy::too_many_arguments)]
fn process_where_clauses(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    obligations: &mut Vec<Obligation>,
    generic_params: &[glyim_hir::GenericParam],
    where_clauses: &[glyim_hir::where_clause::WhereClause],
    _item_span: Span,
) {
    let param_map = tyconv::build_param_tys(ctx, generic_params);

    for wc in where_clauses {
        let ty = tyconv::resolve_type_ref(
            ctx,
            infer,
            def_map,
            diagnostics,
            &wc.ty,
            &param_map,
            wc.span,
        );
        if ty == Ty::ERROR {
            continue;
        }

        for bound in &wc.bounds {
            let trait_path = &bound.trait_path;
            let trait_def_id = match tyconv::resolve_path_to_trait_def_id(def_map, ctx, trait_path, bound.span)
            {
                Some(id) => Some(id),
                None => {
                    let path_str = trait_path
                        .segments
                        .iter()
                        .map(|s| ctx.name_str(s.name))
                        .collect::<Vec<_>>()
                        .join("::");
                    diagnostics.push(GlyimDiagnostic::type_error(
                        bound.span,
                        format!("unresolved trait `{}` in where clause", path_str),
                    ));
                    None
                }
            };

            if let Some(trait_def_id) = trait_def_id {
                let trait_ref = TraitRef {
                    def_id: trait_def_id,
                    substs: ctx.intern_substitution(vec![GenericArg::Ty(ty)]),
                };
                let trait_pred = TraitPredicate {
                    trait_ref,
                    polarity: ImplPolarity::Positive,
                };
                obligations.push(Obligation {
                    predicate: Predicate::Trait(trait_pred),
                    cause: ObligationCause {
                        span: bound.span,
                        code: glyim_solve::ObligationCauseCode::WellFormed,
                    },
                });
            }
        }
    }

}

/// Map an HIR `extern "C"` ABI name (if any) to a `glyim_core::Abi`.
/// `None` (no `extern` qualifier) is the default Glyim ABI. The recognized
/// strings are "C" and "system"; anything else falls back to Glyim
/// (unstub-5 Phase 4).
fn abi_from_name(abi: Option<Name>, ctx: &TyCtxMut) -> Abi {
    match abi {
        None => Abi::Glyim,
        Some(name) => match ctx.name_str(name) {
            "C" | "c" => Abi::C,
            "system" => Abi::System,
            _ => Abi::Glyim,
        },
    }
}

fn find_trait_default_body(
    hir: &glyim_hir::CrateHir,
    trait_ref_path: &Option<glyim_hir::Path>,
    method_name: Name,
) -> Option<glyim_hir::BodyId> {
    let trait_path = trait_ref_path.as_ref()?;
    let trait_name = trait_path.as_name()?;

    for (_item_id, item) in hir.items.iter_enumerated() {
        if let ItemKind::Trait(trait_item) = &item.kind {
            if item.name == trait_name {
                for method in &trait_item.methods {
                    if method.name == method_name {
                        return method.default_body;
                    }
                }
            }
        }
    }
    None
}
#[cfg(test)]
mod tests;

impl TypeckResult {
    /// Resolved type of a HIR expression.
    ///
    /// Returns `None` if `body_id`/`expr_id` was not checked (e.g. the body
    /// had no `Fn` item, or the id is out of range). The type is fully
    /// resolved (no inference variables) — see `typeck_crate`'s post-
    /// fulfillment resolution step.
    pub fn expr_ty(&self, body_id: LocalDefId, expr_id: usize) -> Option<Ty> {
        self.expr_types
            .get(&body_id)
            .and_then(|m| m.get(&ExprId::from_raw(expr_id as u32)))
            .copied()
    }
    /// Resolved type of a HIR pattern.
    ///
    /// Patterns are not yet collected into a per-`PatId` cache during
    /// `typeck_crate`, so this returns `None` rather than a fake value.
    /// Wiring pattern-type collection (mirroring `expr_types`) is a follow-up.
    pub fn pat_ty(&self, _body_id: LocalDefId, _pat_id: usize) -> Option<Ty> {
        None
    }
/// adjustments.
    pub fn adjustments(&self, _body_id: LocalDefId, _expr_id: usize) -> &[Adjustment] {
        &[]
    }
}