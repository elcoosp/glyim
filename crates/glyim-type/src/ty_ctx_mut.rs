use crate::adt_def::*;
use crate::auto_trait::*;
use crate::const_val::*;
use crate::display::TypeLookup;
use crate::flags::*;
use crate::fn_sig::FnSig;
use crate::region::*;
use crate::substitution::*;
use crate::ty::*;
use crate::type_arena::TypeArena;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{AdtId, ClosureId, ConstDefId, CrateId, DefId, FnDefId, LocalDefId, OpaqueTyId};
use glyim_core::interner::{Interner, Name};
use glyim_core::primitives::{IntTy, Mutability, UintTy};
use crate::lang_items::{LangItem, LangItems};
use std::collections::HashMap;
use std::collections::HashSet;

/// TyCtxMut.
pub struct TyCtxMut {
    /// Shared, canonical type table for this compilation (see `type_arena`).
    /// Held as a leaked `&'static` so `Ty`/`Substitution` handles stay valid
    /// across every `TyCtx`/`TyCtxMut` view of the compilation — the fix for
    /// the cross-context handle-validity ("aliasing") class of bugs.
    arena: &'static TypeArena,
    regions: IndexVec<RegionVid, Region>,
    resolver: Interner,
    auto_trait_registry: AutoTraitRegistry,
    deref_registry: crate::deref::DerefRegistry,
    adt_reprs: HashMap<AdtId, AdtRepr>,
    opaque_hidden: HashMap<OpaqueTyId, Ty>,
    interior_mutable_adt_ids: HashSet<AdtId>,
    interior_mutability_cache: HashMap<AdtId, bool>,
    /// Bumped on every `register_adt`. The interior-mutability cache is invalidated
    /// (cleared) whenever the set of registered ADTs changes, so a query made before
    /// a `Cell`-containing field was registered cannot return a stale `false`.
    adt_generation: u64,
    adt_defs: HashMap<AdtId, AdtDef>,
    /// Maps an ADT's (HIR-interned) name to its `AdtId`. Populated alongside
    /// `adt_defs` by `register_adt_with_name` so that ADT *type* resolution
    /// (`resolve_name_to_adt_ty`) can find user ADTs by name even though the
    /// HIR and def-map use distinct interners (the def-map scope is keyed by
    /// the def-map interner, which differs from the HIR/typeck interner).
    /// Plan unstub-5 P5.
    adt_by_name: HashMap<Name, AdtId>,

    pub(crate) trait_defs: HashMap<glyim_core::def_id::TraitDefId, crate::TraitDef>,
    /// Maps a trait's (interned) name to its `TraitDefId`. Populated alongside
    /// `trait_defs` by `register_trait_with_name` so that trait *type*
    /// resolution (`resolve_impl_header`, `resolve_path_to_trait_def_id`) can
    /// find builtin/lang traits (`Future`, `Drop`, `Deref`, `Clone`, …) by name
    /// even though they are not declared in the user's source (plan:
    /// stdlib-compile). Without this, `impl Future for X` / `T: Clone` fail to
    /// resolve because the def-map only contains user-declared traits.
    pub trait_by_name: HashMap<Name, glyim_core::def_id::TraitDefId>,

    /// Maps a type-alias name (`Result`, …) to its formal parameters
    /// `(index, name)` and a *template* `Ty` of the RHS with those formals
    /// left as `TyKind::Param`. Populated from user `type X<..> = ..;`
    /// declarations during pipeline setup so `resolve_name_to_adt_ty` can
    /// expand 1-argument usages (`Result<usize>`) into the full RHS
    /// (`Result<usize, Error>`). stdlib-completion.
    pub type_aliases: HashMap<Name, (Vec<(u32, Name)>, Ty)>,

    variant_types: HashMap<AdtId, Vec<Ty>>,
    fn_sigs: HashMap<FnDefId, FnSig>,
    const_tys: HashMap<ConstDefId, Ty>,
    closure_sigs: HashMap<ClosureId, FnSig>,
    body_tys: HashMap<LocalDefId, Ty>,
    /// Registry of compiler-known builtin types/traits (Option, Range, Drop,
    /// …). The single source of truth that replaces scattered hardcoded
    /// AdtId/DefId constants across the compiler. See `lang_items.rs`.
    lang_items: LangItems,
    /// Counter for compiler-synthesized ADT ids (closures, etc.). Seeded far
    /// above any user-defined ADT id and the 1000-1005 builtin range ids so it
    /// can never collide.
    synthetic_adt_counter: u32,
    /// Maps a closure's `ClosureId` to the synthetic `AdtId` representing its
    /// captured environment. Populated by `register_closure` so downstream
    /// passes (e.g. the DWARF debug-info pass) can recover the per-capture
    /// field types from a `TyKind::Closure` value. Real closure types are
    /// `TyKind::Adt(closure_adt, _)`; this map also covers the `TyKind::Closure`
    /// representation used in tests and downstream reads.
    closure_adt_map: HashMap<ClosureId, AdtId>,
    /// Concrete associated-type projection table: `(impl_self_ty, trait_def_id)`
    /// → `[(assoc_name, defining_ty)]`. Populated during the impl-registration
    /// loop from `HIR ImplItem.associated_types` so that `Self::Output` /
    /// `Type::Output` can be resolved to its defining type (plan unstub-5 P5).
    /// Resolved via `resolve_associated_type`.
    pub(crate) impl_assoc_types:
        HashMap<(Ty, glyim_core::def_id::TraitDefId), Vec<(Name, Ty)>>,

    /// Parameter-name → trait-def-ids for generic params bound by
    /// `T: Trait` (inline `fn f<T: Trait>` or `where T: Trait`). Populated
    /// during `typeck_crate` from every function's `generic_params` /
    /// `where_clauses` so that associated-type projection and method dispatch
    /// on a generic receiver (`F::Output`, `f.poll()`) can find the bound
    /// trait without a full trait solver (plan unstub-5 P5). Name-keyed and
    /// crate-wide; sufficient because parameter names are interned and unique
    /// within a function. Stores `(trait_name, trait_def_id)` so the generic
    /// method-dispatch fallback can match the HIR trait by *name* (avoiding
    /// the `TraitDefId`/`LocalDefId` interner misalignment that otherwise
    /// drops the candidate).
    pub param_bounds: HashMap<Name, Vec<(Name, glyim_core::def_id::TraitDefId)>>,

    /// `AdtId`s with an explicit `Drop` impl (or owning builtins). Mirrors the
    /// `drop_impls` set on the frozen `TyCtx`, consulted by `needs_drop`.
    drop_impls: HashSet<AdtId>,
    /// Concrete trait-method dispatch table (mirrors `TyCtx::impl_method_fns`).
    /// Populated during `typeck_crate` from `impl Trait for Type` items;
    /// carried into the frozen `TyCtx` for monomorphization / interpreter
    /// devirtualization.
    pub(crate) impl_method_fns:
        HashMap<(glyim_core::def_id::TraitDefId, AdtId), HashMap<Name, FnDefId>>,

    /// Builtin inherent-method table: `(receiver_adt_id, method_name)` →
    /// `(synthetic FnDefId, method FnSig)`. Populated in `register_builtin_ranges`
    /// for the builtin collection/enum types (`Vec`, `String`, `Result`,
    /// `Option`, …) whose methods the stdlib calls but which have no user
    /// `impl` block. The FnSig's generic params are expressed as
    /// `TyKind::Param(i)` referring to the *receiver's* generic arguments
    /// (0 = `T`, 1 = `E` for `Result`); the call site instantiates them against
    /// the receiver's substitution. stdlib-completion.
    pub builtin_method_fns: HashMap<(AdtId, Name), (FnDefId, FnSig)>,
    /// Counter for synthetic `FnDefId`s handed out to builtin methods. Seeded
    /// far above user/closure fn ids so it can never collide.
    next_builtin_fn_id: u32,
}

impl TyCtxMut {
/// new.
    pub fn new(resolver: Interner) -> Self {
        // One canonical type arena per compilation, leaked so its handles stay
        // valid across every TyCtx/TyCtxMut derived from this one.
        let arena: &'static TypeArena = Box::leak(Box::new(TypeArena::new()));
        let mut ctx = Self {
            arena,
            regions: IndexVec::new(),
            resolver,
            auto_trait_registry: AutoTraitRegistry::new(),
            deref_registry: crate::deref::DerefRegistry::new(),
            adt_reprs: HashMap::new(),
            opaque_hidden: HashMap::new(),
            interior_mutable_adt_ids: HashSet::new(),
            interior_mutability_cache: HashMap::new(),
            adt_generation: 0,
            adt_defs: HashMap::new(),
            adt_by_name: HashMap::new(),
            trait_defs: HashMap::new(),
            trait_by_name: HashMap::new(),
            type_aliases: HashMap::new(),
            variant_types: HashMap::new(),
            fn_sigs: HashMap::new(),
            const_tys: HashMap::new(),
            closure_sigs: HashMap::new(),
            closure_adt_map: HashMap::new(),
            impl_assoc_types: HashMap::new(),
            param_bounds: HashMap::new(),
            body_tys: HashMap::new(),
            lang_items: LangItems::default(),
            synthetic_adt_counter: 2_000_000,
            drop_impls: HashSet::new(),
            impl_method_fns: HashMap::new(),
            builtin_method_fns: HashMap::new(),
            next_builtin_fn_id: 9_000,
        };
        // sentinels
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Error).to_raw(),
            Ty::ERROR.to_raw(),
            "Ty::ERROR sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Never).to_raw(),
            Ty::NEVER.to_raw(),
            "Ty::NEVER sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Unit).to_raw(),
            Ty::UNIT.to_raw(),
            "Ty::UNIT sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Bool).to_raw(),
            Ty::BOOL.to_raw(),
            "Ty::BOOL sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Uint(UintTy::U8)).to_raw(),
            Ty::U8.to_raw(),
            "Ty::U8 sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Uint(UintTy::U16)).to_raw(),
            Ty::U16.to_raw(),
            "Ty::U16 sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Uint(UintTy::U32)).to_raw(),
            Ty::U32.to_raw(),
            "Ty::U32 sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Uint(UintTy::U64)).to_raw(),
            Ty::U64.to_raw(),
            "Ty::U64 sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Uint(UintTy::Usize)).to_raw(),
            Ty::USIZE.to_raw(),
            "Ty::USIZE sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Int(IntTy::I8)).to_raw(),
            Ty::I8.to_raw(),
            "Ty::I8 sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Int(IntTy::I16)).to_raw(),
            Ty::I16.to_raw(),
            "Ty::I16 sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Int(IntTy::I32)).to_raw(),
            Ty::I32.to_raw(),
            "Ty::I32 sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Int(IntTy::I64)).to_raw(),
            Ty::I64.to_raw(),
            "Ty::I64 sentinel mismatch"
        );
        assert_eq!(
            ctx.alloc_ty_internal(TyKind::Int(IntTy::Isize)).to_raw(),
            Ty::ISIZE.to_raw(),
            "Ty::ISIZE sentinel mismatch"
        );
        ctx.register_builtin_ranges();
        ctx
    }

    /// Build a `TyCtxMut` that shares `ctx`'s canonical type arena (so any type
    /// it allocates is valid in `ctx` and every other view). Carries over the
    /// maps that drop-glue elaboration consults (`drop_impls`, `adt_defs`,
    /// `adt_reprs`, interior-mutability). The synthetic-ADT bookkeeping and
    /// name/param caches are reset to defaults: elaboration does not register
    /// new ADTs by name, so losing that transient state is safe.
    /// Create a mutable view that **shares** `ctx`'s canonical `&'static`
    /// type arena (and all registered defs). Unlike [`TyCtxMut::new`], which
    /// leaks a brand-new arena, this keeps every `Ty`/`Substitution` handle
    /// valid across the `TyCtx`/`TyCtxMut` boundary — essential when the
    /// substituted types must later be read back through the original frozen
    /// `TyCtx` (e.g. during codegen of a monomorphized body). Using `new`
    /// here produces stale handles that manifest as `UnknownType`/`type_arena`
    /// out-of-bounds errors.
    pub fn from_ty_ctx(ctx: &super::ty_ctx::TyCtx) -> Self {
        Self {
            arena: ctx.arena,
            regions: ctx.regions.clone(),
            resolver: ctx.resolver.clone(),
            auto_trait_registry: ctx.auto_trait_registry.clone(),
            deref_registry: ctx.deref_registry.clone(),
            adt_reprs: ctx.adt_reprs.clone(),
            opaque_hidden: ctx.opaque_hidden.clone(),
            interior_mutable_adt_ids: ctx.interior_mutable_adt_ids.clone(),
            interior_mutability_cache: HashMap::new(),
            adt_generation: 0,
            adt_defs: ctx.adt_defs.clone(),
            adt_by_name: ctx.adt_by_name.clone(),
            trait_defs: ctx.trait_defs.clone(),
            trait_by_name: ctx.trait_by_name.clone(),
            type_aliases: HashMap::new(),
            variant_types: ctx.variant_types.clone(),
            fn_sigs: ctx.fn_sigs.clone(),
            const_tys: ctx.const_tys.clone(),
            closure_sigs: ctx.closure_sigs.clone(),
            closure_adt_map: ctx.closure_adt_map.clone(),
            impl_assoc_types: ctx.impl_assoc_types.clone(),
            param_bounds: HashMap::new(),
            body_tys: ctx.body_tys.clone(),
            lang_items: ctx.lang_items.clone(),
            synthetic_adt_counter: 2_000_000,
            drop_impls: ctx.drop_impls.clone(),
            impl_method_fns: ctx.impl_method_fns.clone(),
            builtin_method_fns: HashMap::new(),
            next_builtin_fn_id: 9_000,
        }
    }

    fn alloc_ty_internal(&mut self, kind: TyKind) -> Ty {
        let flags = compute_flags(&kind, self, 0);
        self.arena.alloc_ty(kind, flags)
    }

/// alloc_ty.
    pub fn alloc_ty(&mut self, kind: TyKind) -> Ty {
        self.alloc_ty_internal(kind)
    }

/// ty_kind.
    pub fn ty_kind(&self, ty: Ty) -> &TyKind {
        self.arena.ty_kind(ty)
    }

    /// Mechanically dereference a type for auto-deref in method resolution.
    /// Delegates to the canonical `TyCtx::deref_ty` (de-stubbing plan §9.1).
    pub fn deref_ty(&self, ty: Ty) -> Option<Ty> {
        self.freeze().deref_ty(ty)
    }

/// ty_flags.
    pub fn ty_flags(&self, ty: Ty) -> TypeFlags {
        self.arena.ty_flags(ty)
    }

/// intern_substitution.
    pub fn intern_substitution(&mut self, args: Vec<GenericArg>) -> Substitution {
        self.arena.intern_substitution(args)
    }

/// substitution_args.
    pub fn substitution_args(&self, sub: Substitution) -> &[GenericArg] {
        self.arena.substitution_args(sub)
    }

/// mk_ty.
    pub fn mk_ty(&mut self, kind: TyKind) -> Ty {
        self.alloc_ty(kind)
    }

    /// Substitute generic type parameters (`TyKind::Param`) inside `ty` using
    /// `subst`, which maps a parameter's `index` to its replacement type.
    /// Used for generic call / instantiation: when a generic function
    /// `fn id<T>(x: T) -> T` is called with a concrete argument, each formal
    /// parameter type (which may carry `T`) is matched against the argument
    /// type to build `subst`, and the return type is instantiated through it
    /// (plan unstub-5 P5: generic call instantiation).
    pub fn subst_ty(&mut self, ty: Ty, subst: &std::collections::HashMap<u32, GenericArg>) -> Ty {
        if subst.is_empty() {
            return ty;
        }
        let kind = self.ty_kind(ty).clone();
        
        match kind {
            TyKind::Param(pt) => {
                if let Some(repl) = subst.get(&pt.index) {
                    match repl {
                        GenericArg::Ty(t) => *t,
                        // Const replacements only apply to array lengths (handled
                        // in the `Array` arm); a type parameter is always
                        // substituted by a type, not a const.
                        GenericArg::Const(_) => ty,
                        GenericArg::Lifetime(_) => ty,
                    }
                } else {
                    ty
                }
            }
            TyKind::Ref(region, inner, mutability) => {
                let new_inner = self.subst_ty(inner, subst);
                self.mk_ref(region, new_inner, mutability)
            }
            TyKind::Adt(adt_id, substs) => {
                let args: Vec<GenericArg> = self.substitution_args(substs).to_vec();
                let new_args: Vec<GenericArg> = args
                    .into_iter()
                    .map(|a| match a {
                        GenericArg::Ty(t) => GenericArg::Ty(self.subst_ty(t, subst)),
                        other => other,
                    })
                    .collect();
                let new_substs = self.intern_substitution(new_args);
                self.mk_adt(adt_id, new_substs)
            }
            TyKind::Tuple(substs) => {
                let args: Vec<GenericArg> = self.substitution_args(substs).to_vec();
                let new_args: Vec<GenericArg> = args
                    .into_iter()
                    .map(|a| match a {
                        GenericArg::Ty(t) => GenericArg::Ty(self.subst_ty(t, subst)),
                        other => other,
                    })
                    .collect();
                let new_substs = self.intern_substitution(new_args);
                self.mk_tuple(new_substs)
            }
            TyKind::FnDef(id, substs) => {
                let args: Vec<GenericArg> = self.substitution_args(substs).to_vec();
                let new_args: Vec<GenericArg> = args
                    .into_iter()
                    .map(|a| match a {
                        GenericArg::Ty(t) => GenericArg::Ty(self.subst_ty(t, subst)),
                        other => other,
                    })
                    .collect();
                let new_substs = self.intern_substitution(new_args);
                self.mk_ty(TyKind::FnDef(id, new_substs))
            }
            TyKind::Projection(proj) => {
                // Plan unstub-5 P5: substitute the projection's self type. When
                // the substituted self type becomes a concrete ADT that has a
                // registered impl for the bound trait, normalize the projection
                // to the concrete associated type (e.g. `F::Output` with
                // `F = AddOne` -> `i32`). This is what lets `block_on<F>`'s
                // return type `F::Output` resolve to the caller's concrete type
                // at the call site.
                let trait_ref = proj.trait_ref;
                let self_ty = self.substitution_args(trait_ref.substs).first().cloned();
                let new_self = match self_ty {
                    Some(GenericArg::Ty(t)) => self.subst_ty(t, subst),
                    Some(_) | None => {
                        // No self type to substitute; keep the projection as-is.
                        return ty;
                    }
                };
                // If the self type is unchanged (no substitution applied), keep
                // the original projection.
                let changed = match self_ty {
                    Some(GenericArg::Ty(orig)) => orig != new_self,
                    _ => false,
                };
                if !changed {
                    return ty;
                }
                let new_args = vec![GenericArg::Ty(new_self)];
                let new_substs = self.intern_substitution(new_args);
                let new_trait_ref = crate::predicate::TraitRef {
                    def_id: trait_ref.def_id,
                    substs: new_substs,
                };
                let new_proj = ProjectionTy {
                    trait_ref: new_trait_ref,
                    item_name: proj.item_name,
                };
                // Attempt normalization: only succeeds when `new_self` is a
                // concrete type with a registered associated-type impl.
                if let Some(resolved) =
                    self.resolve_associated_type(new_self, trait_ref.def_id, proj.item_name)
                {
                    // The impl's associated type is frequently expressed in terms
                    // of the impl's own generic parameters (e.g.
                    // `impl<T> Future for Ready<T> { type Output = T }` registers
                    // `Output = T`). After resolving we must substitute those impl
                    // parameters with the concrete type arguments carried by
                    // `new_self` (e.g. `Ready<i32>` -> `T := i32`), otherwise the
                    // projection normalizes to a dangling `Param` and unification
                    // against the caller's concrete type fails.
                    let impl_subst = self.impl_param_subst(new_self, trait_ref.def_id, proj.item_name);
                    if !impl_subst.is_empty() {
                        self.subst_ty(resolved, &impl_subst)
                    } else {
                        resolved
                    }
                } else {
                    self.mk_ty(TyKind::Projection(new_proj))
                }
            }
            TyKind::Array(inner, len) => {
                // Phase 2 (GLYIM_DESTUB_PLAN): substitute both the element type
                // AND a const-parameter length. Previously the length was left
                // unchanged, so `[T; N]` with `N` a `ConstKind::Param` could not
                // be monomorphized to a concrete length.
                let new_inner = self.subst_ty(inner, subst);
                let new_len = match &len.kind {
                    ConstKind::Param(pc) => {
                        if let Some(GenericArg::Const(c)) = subst.get(&pc.index) {
                            c.clone()
                        } else {
                            len.clone()
                        }
                    }
                    _ => len.clone(),
                };
                self.mk_ty(TyKind::Array(new_inner, new_len))
            }
            TyKind::Slice(inner) => {
                let new_inner = self.subst_ty(inner, subst);
                self.mk_ty(TyKind::Slice(new_inner))
            }
            _ => ty,
        }
    }

/// mk_ref.
    pub fn mk_ref(&mut self, region: Region, ty: Ty, mutability: Mutability) -> Ty {
        self.mk_ty(TyKind::Ref(region, ty, mutability))
    }

/// mk_adt.
    pub fn mk_adt(&mut self, adt_id: AdtId, substs: Substitution) -> Ty {
        self.mk_ty(TyKind::Adt(adt_id, substs))
    }

/// mk_tuple.
    pub fn mk_tuple(&mut self, substs: Substitution) -> Ty {
        self.mk_ty(TyKind::Tuple(substs))
    }

/// mk_fn_ptr.
    pub fn mk_fn_ptr(&mut self, sig: FnSig) -> Ty {
        self.mk_ty(TyKind::FnPtr(sig))
    }

/// error_ty.
    pub fn error_ty(&self) -> Ty {
        Ty::ERROR
    }

/// never_ty.
    pub fn never_ty(&self) -> Ty {
        Ty::NEVER
    }

/// unit_ty.
    pub fn unit_ty(&self) -> Ty {
        Ty::UNIT
    }

/// bool_ty.
    pub fn bool_ty(&self) -> Ty {
        Ty::BOOL
    }

/// resolver.
    pub fn resolver(&self) -> &Interner {
        &self.resolver
    }

/// name_str.
    pub fn name_str(&self, name: Name) -> &str {
        self.resolver.resolve(name)
    }

/// is_copy.
    pub fn is_copy(&self, ty: Ty) -> bool {
        match self.ty_kind(ty) {
            TyKind::Bool | TyKind::Int(_) | TyKind::Uint(_) | TyKind::Float(_) | TyKind::Char => {
                true
            }
            TyKind::Never | TyKind::Unit => true,
            TyKind::Ref(_, _, _) => false,
            TyKind::RawPtr(_, _) => false,
            TyKind::Slice(_) => false,
            TyKind::Array(inner, _) => self.is_copy(*inner),
            TyKind::Tuple(substs) => {
                for arg in self.substitution_args(*substs) {
                    if let GenericArg::Ty(t) = arg
                        && !self.is_copy(*t)
                    {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }

/// new_region_var.
    pub fn new_region_var(&mut self, initial: Region) -> RegionVid {
        self.regions.push(initial)
    }

/// region_var.
    pub fn region_var(&self, vid: RegionVid) -> &Region {
        &self.regions[vid]
    }

/// region_var_count.
    pub fn region_var_count(&self) -> usize {
        self.regions.len()
    }

/// register_adt_repr.
    pub fn register_adt_repr(&mut self, adt_id: AdtId, field_tys: Vec<Ty>) {
        self.adt_reprs.insert(adt_id, AdtRepr::new(field_tys));
    }

/// register_negative_impl.
    pub fn register_negative_impl(&mut self, adt_id: AdtId, auto_trait: AutoTrait) {
        self.auto_trait_registry
            .register_negative_impl(adt_id, auto_trait);
    }

/// register_manual_impl.
    pub fn register_manual_impl(&mut self, adt_id: AdtId, auto_trait: AutoTrait) {
        self.auto_trait_registry
            .register_manual_impl(adt_id, auto_trait);
    }

/// register_deref_impl (Phase 5 GLYIM_DESTUB_PLAN).
    pub fn register_deref_impl(&mut self, self_ty: Ty, target_ty: Ty) {
        self.deref_registry.register_deref_impl(self_ty, target_ty);
        // Also register a generic template keyed by the self ADT id so that
        // concrete queries (e.g. `Box<Vec<i32>>`) match
        // `impl<T> Deref for Box<T> { type Target = T; }`.
        if let TyKind::Adt(adt_id, sub) = self.ty_kind(self_ty) {
            let self_params: Vec<GenericArg> = self.substitution_args(*sub).to_vec();
            self.deref_registry
                .register_deref_template(*adt_id, self_params, target_ty);
        }
    }

/// register_adt.
    pub fn register_adt(&mut self, id: AdtId, def: AdtDef) {
        // Compute variant types from variants
        let variant_tys: Vec<Ty> = def
            .variants
            .iter()
            .map(|variant| {
                let field_tys: Vec<Ty> = variant.fields.iter().map(|f| f.ty).collect();
                if field_tys.is_empty() {
                    self.unit_ty()
                } else if field_tys.len() == 1 {
                    field_tys[0]
                } else {
                    let args: Vec<GenericArg> = field_tys.into_iter().map(GenericArg::Ty).collect();
                    let subst = self.intern_substitution(args);
                    self.mk_ty(TyKind::Tuple(subst))
                }
            })
            .collect();
        self.variant_types.insert(id, variant_tys);
        self.adt_defs.insert(id, def.clone());
        // Registering a new ADT may change interior-mutability answers for any
        // previously-queried ADT that transitively references it. Invalidate the
        // cache so no stale `false` survives (see §1.2).
        self.adt_generation += 1;
        self.interior_mutability_cache.clear();
        if self.compute_adt_interior_mutability(id) {
            self.mark_adt_interior_mutable(id);
        }
    }

    /// Register an ADT together with its (HIR-interned) name, so that ADT *type*
    /// resolution by name (`resolve_name_to_adt_ty`) can find it regardless of
    /// the def-map interner (plan unstub-5 P5).
    pub fn register_adt_with_name(&mut self, name: Name, id: AdtId, def: AdtDef) {
        self.adt_by_name.insert(name, id);
        self.register_adt(id, def);
    }

    /// Allocate a fresh compiler-synthesized `AdtId` that cannot collide with
    /// user-defined ADTs or the builtin fixed-id ADTs (ranges, etc.).
    pub fn next_synthetic_adt_id(&mut self) -> AdtId {
        let id = self.synthetic_adt_counter;
        self.synthetic_adt_counter += 1;
        AdtId::from_raw(id)
    }

    /// Register a type alias (`type X<..> = RHS;`) for expansion during type
    /// resolution. `params` carries each formal's `(index, name)`; `template`
    /// is the RHS type with those formals left as `TyKind::Param` so call
    /// sites can substitute concrete arguments via `subst_ty` (stdlib-completion:
    /// `type Result<T> = Result<T, Error>` expands `Result<usize>` → `Result<usize, Error>`).
    pub fn register_type_alias(&mut self, name: Name, params: Vec<(u32, Name)>, template: Ty) {
        self.type_aliases.insert(name, (params, template));
    }

    /// Look up a registered type alias by name (see `register_type_alias`).
    pub fn lookup_type_alias(&self, name: Name) -> Option<&(Vec<(u32, Name)>, Ty)> {
        self.type_aliases.get(&name)
    }

    /// Register a nominal ADT representing a closure's captured environment.
    ///
    /// Each distinct closure expression (with a distinct capture set) gets its
    /// own synthetic ADT with one field per captured variable. This gives a
    /// closure a real, concrete `TyKind::Adt` so downstream passes (MIR
    /// lowering, layout, codegen) have a genuine type to work with instead of
    /// an unconstrained inference variable.
    pub fn register_closure(&mut self, capture_tys: Vec<(Name, Ty)>) -> AdtId {
        let id = self.next_synthetic_adt_id();
        let mut field_defs = IndexVec::new();
        for (name, ty) in capture_tys.iter() {
            field_defs.push(FieldDef {
                name: *name,
                ty: *ty,
            });
        }
        let def = AdtDef {
            kind: AdtKind::Struct,
            fields: field_defs.clone(),
            variants: vec![VariantDef {
                name: self.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                fields: field_defs,
            }],
            generic_params: vec![],
};
        self.register_adt(id, def);
        // Record the 1:1 mapping from the synthetic ClosureId (derived from the
        // ADT id) to the ADT so the DWARF debug pass can recover per-capture
        // member types from a `TyKind::Closure` value (Phase 6.2).
        self.closure_adt_map
            .insert(ClosureId::from_raw(id.to_raw()), id);
        id
    }
/// adt_def.
    pub fn adt_def(&self, id: AdtId) -> Option<&AdtDef> {
        self.adt_defs.get(&id)
    }

    /// Look up a registered ADT by its (HIR-interned) name. Used by type
    /// resolution so user ADTs resolve regardless of the def-map interner
    /// (plan unstub-5 P5).
    pub fn adt_id_by_name(&self, name: Name) -> Option<AdtId> {
        self.adt_by_name.get(&name).copied()
    }

    /// Resolve a bare variant constructor/value name (`Ok`, `Err`, `Some`,
    /// `None`, `Ready`, …) to its `(AdtId, VariantIdx)`. The stdlib/pre Rust
    /// prelude references enum variants by their bare name (e.g. `Ok(x)`),
    /// not the qualified `Enum::Variant` form, so value-path resolution must
    /// search every registered enum's variant list. Returns the first match
    /// (variant names are unique across the builtins the stdlib relies on).
    pub fn variant_by_name(&self, name: Name) -> Option<(AdtId, glyim_core::def_id::VariantIdx)> {
        for (adt_id, def) in self.adt_defs.iter() {
            for (i, v) in def.variants.iter().enumerate() {
                if v.name == name {
                    return Some((*adt_id, glyim_core::def_id::VariantIdx::from_raw(i as u32)));
                }
            }
        }
        None
    }

    /// Number of generic type parameters declared on `adt_id`
    /// (`struct S<T, U>` → 2). Returns 0 for non-generic ADTs and unknown ids.
    /// Drives substitution-arity checking in type resolution (unstub-5 P1.4).
    pub fn adt_generic_arity(&self, adt_id: AdtId) -> usize {
        self.adt_defs
            .get(&adt_id)
            .map(|d| d.generic_params.len())
            .unwrap_or(0)
    }

/// field_index.
    pub fn field_index(&self, adt_id: AdtId, field_name: Name) -> Option<usize> {
        if let Some(def) = self.adt_defs.get(&adt_id) {
            for (i, field) in def.fields.iter_enumerated() {
                if field.name == field_name {
                    return Some(i.index());
                }
            }
        }
        None
    }

/// field_ty.
    pub fn field_ty(&self, adt_id: AdtId, field_idx: usize) -> Ty {
        if let Some(def) = self.adt_defs.get(&adt_id) {
            return def
                .fields
                .as_slice()
                .get(field_idx)
                .map(|f| f.ty)
                .unwrap_or_else(|| self.error_ty());
        }
        if let Some(repr) = self.adt_reprs.get(&adt_id) {
            return repr
                .field_tys
                .get(field_idx)
                .copied()
                .unwrap_or_else(|| self.error_ty());
        }
        self.error_ty()
    }

/// register_fn_sig.
    pub fn register_fn_sig(&mut self, def_id: FnDefId, sig: FnSig) {
        self.fn_sigs.insert(def_id, sig);
    }

    /// Register the value type of a constant definition so `check_path` can
    /// resolve a `const` reference to a `thir::ExprKind::ConstRef` with the
    /// right type (plan: value-namespace const paths).
    pub fn register_const_ty(&mut self, def_id: ConstDefId, ty: Ty) {
        self.const_tys.insert(def_id, ty);
    }

    /// Register a trait definition (its name and method set) so that vtable
    /// generation and other downstream passes can resolve the trait's methods.
    pub fn register_trait_def(&mut self, id: glyim_core::def_id::TraitDefId, def: crate::TraitDef) {
        self.trait_defs.insert(id, def);
    }

    /// Register a concrete impl's associated-type definitions for projection
    /// resolution (plan unstub-5 P5). `self_ty` + `trait_def_id` key the entry;
    /// each `(assoc_name, defining_ty)` is the resolved `type X = ...;`.
    pub fn register_impl_assoc_types(
        &mut self,
        self_ty: Ty,
        trait_def_id: glyim_core::def_id::TraitDefId,
        assoc_types: Vec<(Name, Ty)>,
    ) {
        self.impl_assoc_types.insert((self_ty, trait_def_id), assoc_types);
    }

    /// Reverse-lookup variant of `resolve_associated_type` that finds the entry
    /// by `self_ty` + assoc name alone (no trait needed); used for concrete
    /// `Type::Item` paths (plan unstub-5 P5).
    ///
    /// Matching is by ADT identity (`AdtId`) when `self_ty` is an ADT, so that
    /// distinct `Ty` handles with the same underlying struct/enum (e.g.
    /// differing only in their `Substitution` index) still resolve. This is the
    /// correct semantics: an impl's associated type is keyed by the self type's
    /// ADT, not by a specific substitution.
    pub fn resolve_associated_type_by_self_ty(
        &self,
        self_ty: Ty,
        assoc_name: Name,
    ) -> Option<Ty> {
        let self_adt = match self.ty_kind(self_ty) {
            TyKind::Adt(adt_id, _) => Some(*adt_id),
            _ => None,
        };
        self.impl_assoc_types
            .iter()
            .find(|((sty, _), entries)| {
                let sty_matches = match self_adt {
                    Some(a) => matches!(self.ty_kind(*sty), TyKind::Adt(b, _) if b == &a),
                    None => *sty == self_ty,
                };
                sty_matches && entries.iter().any(|(n, _)| *n == assoc_name)
            })
            .and_then(|((_, _), entries)| {
                entries
                    .iter()
                    .find(|(name, _)| *name == assoc_name)
                    .map(|(_, ty)| *ty)
            })
    }

    /// Structural ADT-type equality with the arena: two `Ty`s that both denote
    /// the same `Adt(adt_id, _)` compare equal even when they are distinct arena
    /// allocations. Used to key associated-type lookups by logical type rather
    /// than by `Ty` handle (plan unstub-5 P5).
    #[allow(dead_code)]
    fn same_adt(&self, a: Ty, b: Ty) -> bool {
        match (self.ty_kind(a), self.ty_kind(b)) {
            (TyKind::Adt(id_a, _), TyKind::Adt(id_b, _)) => id_a == id_b,
            _ => a == b,
        }
    }

    /// Resolve an associated-type projection `Self::Item` / `Type::Item` to its
    /// defining type, given the concrete `self_ty` and the trait it's bound to.
    /// Returns `None` when no matching impl registration exists (the abstract /
    /// generic-param case, which requires the full trait solver).
    pub fn resolve_associated_type(
        &self,
        self_ty: Ty,
        trait_def_id: glyim_core::def_id::TraitDefId,
        assoc_name: Name,
    ) -> Option<Ty> {
        // Exact (self_ty, trait) key match. `Ty` equality is by interned raw
        // index; the frozen `TyCtx` (used by codegen) adds `by_adt` /
        // param-placeholder fallbacks, but the mutation phase only needs the
        // exact registration. A single registered entry is keyed here, so the
        // inner `find` can never be ambiguous — no first-match hazard.
        let exact = self
            .impl_assoc_types
            .get(&(self_ty, trait_def_id))
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|(name, _)| *name == assoc_name)
                    .map(|(_, ty)| *ty)
            });
        if exact.is_some() {
            return exact;
        }
        // By-ADT fallback: an impl's associated type is keyed by the self type's
        // ADT, not a specific `Ty` handle. When `self_ty` is a concrete
        // instantiation of a *generic* impl (e.g. `ReadyT<i32>` matching
        // `impl<T> Future for ReadyT<T>`), the exact key misses but the ADT
        // matches — resolve to the impl's associated type, which
        // `subst_ty`'s `impl_param_subst` then specializes (T := i32).
        let self_adt = match self.ty_kind(self_ty) {
            TyKind::Adt(id, _) => Some(*id),
            _ => None,
        };
        if let Some(adt) = self_adt {
            for ((sty, tid), entries) in self.impl_assoc_types.iter() {
                if *tid != trait_def_id {
                    continue;
                }
                let matches = matches!(self.ty_kind(*sty), TyKind::Adt(b, _) if b == &adt)
                    && entries.iter().any(|(n, _)| *n == assoc_name);
                if matches {
                    return entries
                        .iter()
                        .find(|(name, _)| *name == assoc_name)
                        .map(|(_, ty)| *ty);
                }
            }
        }
        None
    }

    /// Build a substitution that replaces an impl's own generic parameters with
    /// the concrete type arguments carried by `concrete_self` (`new_self`).
    ///
    /// Example: for `impl<T> Future for Ready<T> { type Output = T }` the assoc
    /// type registers `Output = T` where `T` is the impl's parameter. When we
    /// normalize `Ready<i32>::Output`, `concrete_self` is `Ready<i32>`; this
    /// finds the impl's `Self` pattern `Ready<T>`, aligns `T` (position 0) with
    /// `i32` (position 0 of `Ready<i32>`), and returns `{ T.index -> i32 }`. The
    /// caller applies it so the normalized type becomes `i32` rather than the
    /// dangling `T`.
    pub(crate) fn impl_param_subst(
        &self,
        concrete_self: Ty,
        trait_def_id: glyim_core::def_id::TraitDefId,
        _assoc_name: Name,
    ) -> std::collections::HashMap<u32, GenericArg> {
        let mut map = std::collections::HashMap::new();
        let concrete_args: Vec<GenericArg> = match self.ty_kind(concrete_self) {
            TyKind::Adt(_, substs) => self.substitution_args(*substs).to_vec(),
            TyKind::Tuple(substs) => self.substitution_args(*substs).to_vec(),
            _ => return map,
        };
        // Find the impl registered for this trait whose `Self` ADT matches.
        for ((impl_self, tid), _) in self.impl_assoc_types.iter() {
            if *tid != trait_def_id {
                continue;
            }
            let impl_args: Option<Vec<GenericArg>> = match self.ty_kind(*impl_self) {
                TyKind::Adt(_, substs) => Some(self.substitution_args(*substs).to_vec()),
                TyKind::Tuple(substs) => Some(self.substitution_args(*substs).to_vec()),
                _ => None,
            };
            let Some(impl_args) = impl_args else { continue };
            // ADT identity already established by the trait key + substs match
            // below; require the same arity then align param positions.
            if impl_args.len() != concrete_args.len() {
                continue;
            }
            let mut matched = true;
            for (ia, ca) in impl_args.iter().zip(concrete_args.iter()) {
                match ia {
                    GenericArg::Ty(it) if matches!(self.ty_kind(*it), TyKind::Param(_)) => {
                        if let TyKind::Param(p) = self.ty_kind(*it) {
                            map.insert(p.index, ca.clone());
                        }
                    }
                    _ => {
                        // Non-param impl arg (e.g. concrete) must match the
                        // concrete argument positionally; otherwise this impl is
                        // not the one we want.
                        if ia != ca {
                            matched = false;
                            break;
                        }
                    }
                }
            }
            if matched {
                return map;
            }
        }
        map
    }

    /// Query the traits a generic parameter (by name) is bound to. Populated
    /// during `typeck_crate` from `generic_params` / `where_clauses` (plan
    /// unstub-5 P5). Used for associated-type projection and method dispatch
    /// on a generic receiver.
    pub fn param_bounds_for(&self, name: Name) -> Option<&[(Name, glyim_core::def_id::TraitDefId)]> {
        self.param_bounds.get(&name).map(|v| v.as_slice())
    }

    /// Find a registered trait definition that declares an associated type
    /// with the given name. Used to resolve `Self::Item` to a projection
    /// against the enclosing trait (plan unstub-5 P5). Returns the first match.
    pub fn find_trait_with_assoc_type(&self, assoc_name: Name) -> Option<glyim_core::def_id::TraitDefId> {
        self.trait_defs
            .iter()
            .find(|(_, def)| def.associated_types.contains(&assoc_name))
            .map(|(id, _)| *id)
    }

    /// Get the trait definition by ID (read-only view into the type context).
    pub fn trait_def(&self, id: glyim_core::def_id::TraitDefId) -> Option<&crate::TraitDef> {
        self.trait_defs.get(&id)
    }

/// fn_sig.
    pub fn fn_sig(&self, def_id: FnDefId) -> Option<&FnSig> {
        self.fn_sigs.get(&def_id)
    }

    /// Value type of a constant definition (mirrors `fn_sig` for consts).
    pub fn const_ty(&self, def_id: ConstDefId) -> Option<Ty> {
        self.const_tys.get(&def_id).copied()
    }

/// register_closure_sig.
    pub fn register_closure_sig(&mut self, closure_id: ClosureId, sig: FnSig) {
        self.closure_sigs.insert(closure_id, sig);
    }

/// closure_sig.
    pub fn closure_sig(&self, closure_id: ClosureId) -> Option<&FnSig> {
        self.closure_sigs.get(&closure_id)
    }

    /// Recover the synthetic `AdtId` representing a closure's captured
    /// environment. Used by the DWARF debug-info pass to emit real per-capture
    /// member types for a `TyKind::Closure` value.
    pub fn closure_adt(&self, closure_id: ClosureId) -> Option<AdtId> {
        self.closure_adt_map.get(&closure_id).copied()
    }

/// register_body_ty.
    pub fn register_body_ty(&mut self, def_id: LocalDefId, ty: Ty) {
        self.body_tys.insert(def_id, ty);
    }

/// body_ty.
    pub fn body_ty(&self, def_id: LocalDefId) -> Option<Ty> {
        self.body_tys.get(&def_id).copied()
    }

/// freeze.
    pub fn freeze(&self) -> super::ty_ctx::TyCtx {
        super::ty_ctx::TyCtx {
            arena: self.arena,
            regions: self.regions.clone(),
            resolver: self.resolver.clone(),
            auto_trait_registry: self.auto_trait_registry.clone(),
            deref_registry: self.deref_registry.clone(),
            adt_reprs: self.adt_reprs.clone(),
            opaque_hidden: self.opaque_hidden.clone(),
            interior_mutable_adt_ids: self.interior_mutable_adt_ids.clone(),
            adt_defs: self.adt_defs.clone(),
            adt_by_name: self.adt_by_name.clone(),
            trait_defs: self.trait_defs.clone(),
            trait_by_name: self.trait_by_name.clone(),
            variant_types: self.variant_types.clone(),
            fn_sigs: self.fn_sigs.clone(),
            const_tys: self.const_tys.clone(),
            closure_sigs: self.closure_sigs.clone(),
            closure_adt_map: self.closure_adt_map.clone(),
            impl_assoc_types: self.impl_assoc_types.clone(),
            body_tys: self.body_tys.clone(),
            lang_items: self.lang_items.clone(),
            drop_impls: self.drop_impls.clone(),
            impl_method_fns: self.impl_method_fns.clone(),
        }
    }

/// freeze_owned.
    pub fn freeze_owned(self) -> super::ty_ctx::TyCtx {
        super::ty_ctx::TyCtx {
            arena: self.arena,
            regions: self.regions,
            resolver: self.resolver,
            auto_trait_registry: self.auto_trait_registry,
            deref_registry: self.deref_registry,
            adt_reprs: self.adt_reprs,
            opaque_hidden: self.opaque_hidden,
            interior_mutable_adt_ids: self.interior_mutable_adt_ids,
            adt_defs: self.adt_defs,
            adt_by_name: self.adt_by_name,
            trait_defs: self.trait_defs.clone(),
            trait_by_name: self.trait_by_name.clone(),
            variant_types: self.variant_types,
            fn_sigs: self.fn_sigs,
            const_tys: self.const_tys,
            closure_sigs: self.closure_sigs,
            closure_adt_map: self.closure_adt_map,
            impl_assoc_types: self.impl_assoc_types,
            body_tys: self.body_tys,
            lang_items: self.lang_items,
            drop_impls: self.drop_impls,
            impl_method_fns: self.impl_method_fns,
        }
    }

    /// Mutable access to the language-item registry (builtin `Option`/`Range`/`Drop`/…).
    pub fn lang_items_mut(&mut self) -> &mut LangItems {
        &mut self.lang_items
    }

/// mark_adt_interior_mutable.
    pub fn mark_adt_interior_mutable(&mut self, adt_id: AdtId) {
        self.interior_mutable_adt_ids.insert(adt_id);
        self.interior_mutability_cache.insert(adt_id, true);
    }

    /// Record the concrete hidden type for an opaque type (`impl Trait` /
    /// `type X = impl Trait` position) at its defining use. Consulted by
    /// auto-trait computation (§7.2) so an opaque type delegates its auto-trait
    /// set to its underlying concrete type instead of being assumed to have
    /// none.
    pub fn register_opaque_hidden(&mut self, opaque_id: OpaqueTyId, hidden: Ty) {
        self.opaque_hidden.insert(opaque_id, hidden);
    }

    /// Record that `adt_id` has an explicit `Drop` impl (or is an owning
    /// builtin such as `String`/`Vec`/`Box`). Consulted by the unified
    /// `TyCtx::needs_drop`; this is the single place drop-carrying-ness is
    /// declared, replacing the previously-divergent per-crate guesses.
    pub fn mark_has_drop(&mut self, adt_id: AdtId) {
        self.drop_impls.insert(adt_id);
    }

    /// Register that `impl trait_def_id for <adt_id>` provides `method_name`
    /// via the function `fn_def_id`. Populated during `typeck_crate` from
    /// `impl Trait for Type` items; carried into the frozen `TyCtx` for
    /// monomorphization / interpreter devirtualization.
    pub fn register_impl_method(
        &mut self,
        trait_def_id: glyim_core::def_id::TraitDefId,
        adt_id: AdtId,
        method_name: Name,
        fn_def_id: FnDefId,
    ) {
        self.impl_method_fns
            .entry((trait_def_id, adt_id))
            .or_default()
            .insert(method_name, fn_def_id);
    }

    fn compute_adt_interior_mutability(&mut self, adt_id: AdtId) -> bool {
        if let Some(&cached) = self.interior_mutability_cache.get(&adt_id) {
            return cached;
        }
        if self.interior_mutable_adt_ids.contains(&adt_id) {
            self.interior_mutability_cache.insert(adt_id, true);
            return true;
        }
        let def = match self.adt_defs.get(&adt_id) {
            Some(def) => def.clone(),
            None => {
                self.interior_mutability_cache.insert(adt_id, false);
                return false;
            }
        };
        let result = self.compute_adt_interior_mutability_with_def(adt_id, &def);
        self.interior_mutability_cache.insert(adt_id, result);
        result
    }

    fn compute_adt_interior_mutability_with_def(&mut self, adt_id: AdtId, def: &AdtDef) -> bool {
        let mut visiting = HashSet::new();
        self.compute_adt_interior_mutability_rec(adt_id, def, &mut visiting)
    }

    fn compute_adt_interior_mutability_rec(
        &mut self,
        adt_id: AdtId,
        def: &AdtDef,
        visiting: &mut HashSet<AdtId>,
    ) -> bool {
        if visiting.contains(&adt_id) {
            return false;
        }
        visiting.insert(adt_id);
        for field in def.fields.iter() {
            let field_ty = field.ty;
            let flags = self.ty_flags(field_ty);
            if flags.contains(TypeFlags::HAS_INTERIOR_MUTABILITY) {
                visiting.remove(&adt_id);
                return true;
            }
            if let TyKind::Adt(child_adt_id, _) = self.ty_kind(field_ty)
                && self.compute_adt_interior_mutability(*child_adt_id)
            {
                visiting.remove(&adt_id);
                return true;
            }
        }
        visiting.remove(&adt_id);
        false
    }

    /// Register built-in range types as ADTs.
    pub fn register_builtin_ranges(&mut self) {
        use crate::adt_def::{AdtDef, AdtKind, FieldDef, VariantDef};
        use glyim_core::arena::IndexVec;
        use glyim_core::def_id::AdtId;

        // Builtin ADTs live in the implicit builtin/core crate (krate 0). Map a
        // local id to its `DefId` for the lang-item registry.
        let def_id = |n: u32| DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(n));

        // Helper to create a struct AdtDef with given field types.
        // This is a plain function, not a closure, so it doesn't capture self.
        // But we can't define a nested function that uses self.resolver directly.
        // We'll just create each def manually.

        // Create a type variable for T.
        let t_var = self.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: self.resolver.intern("T"),
        }));

        // Helper to make field defs for a list of types.
        // We'll call self.resolver.intern directly inside the loop.
        // This is safe because we don't hold any borrows across calls.
        let make_field_defs = |fields: Vec<Ty>, this: &mut Self| {
            let mut field_defs = IndexVec::new();
            for (i, ty) in fields.iter().enumerate() {
                field_defs.push(FieldDef {
                    name: this.resolver.intern(&format!("field_{}", i)),
                    ty: *ty,
                });
            }
            field_defs
        };

        // Helper to make a struct AdtDef.
        let make_struct_def = |fields: Vec<Ty>, this: &mut Self| {
            let field_defs = make_field_defs(fields, this);
            let field_defs_clone = field_defs.clone();
            AdtDef {
                kind: AdtKind::Struct,
                fields: field_defs,
                variants: vec![VariantDef {
                    name: this.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: field_defs_clone,
                }],
                generic_params: vec![],
}
        };

        // Register Range<T> (start, end) - ID 1000
        let def = make_struct_def(vec![t_var, t_var], self);
        self.register_adt(AdtId::from_raw(1000), def);
        self.lang_items.register(LangItem::Range, def_id(1000))
            .expect("builtin lang item registration must not duplicate");

        // Register RangeInclusive<T> (start, end) - ID 1001
        let def = make_struct_def(vec![t_var, t_var], self);
        self.register_adt(AdtId::from_raw(1001), def);
        self.lang_items.register(LangItem::RangeInclusive, def_id(1001))
            .expect("builtin lang item registration must not duplicate");

        // Register RangeFrom<T> (start) - ID 1002
        let def = make_struct_def(vec![t_var], self);
        self.register_adt(AdtId::from_raw(1002), def);
        self.lang_items.register(LangItem::RangeFrom, def_id(1002))
            .expect("builtin lang item registration must not duplicate");

        // Register RangeTo<T> (end) - ID 1003
        let def = make_struct_def(vec![t_var], self);
        self.register_adt(AdtId::from_raw(1003), def);
        self.lang_items.register(LangItem::RangeTo, def_id(1003))
            .expect("builtin lang item registration must not duplicate");

        // Register RangeToInclusive<T> (end) - ID 1004
        let def = make_struct_def(vec![t_var], self);
        self.register_adt(AdtId::from_raw(1004), def);
        self.lang_items.register(LangItem::RangeToInclusive, def_id(1004))
            .expect("builtin lang item registration must not duplicate");

        // Register UnsafeCell<T> - ID 1005
        // This is a special ADT for interior mutability.
        let unsafe_cell_t_var = self.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: self.resolver.intern("T"),
        }));
        let unsafe_cell_def = {
            let mut field_defs = IndexVec::new();
            // UnsafeCell has a single field: value of type T.
            field_defs.push(FieldDef {
                name: self.resolver.intern("value"),
                ty: unsafe_cell_t_var,
            });
            let field_defs_clone = field_defs.clone();
            AdtDef {
                kind: AdtKind::Struct,
                fields: field_defs,
                variants: vec![VariantDef {
                    name: self.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: field_defs_clone,
                }],
                generic_params: vec![],
}
        };
        self.register_adt(AdtId::from_raw(1005), unsafe_cell_def);
        // Mark it as interior mutable.
        self.mark_adt_interior_mutable(AdtId::from_raw(1005));

        // Register Option<T> - ID 1010
        // A two-variant enum: `None` (no fields) and `Some(T)`.
        let none_fields = IndexVec::new();
        let mut some_fields = IndexVec::new();
        some_fields.push(FieldDef {
            name: self.resolver.intern("0"),
            ty: t_var,
        });
        let option_def = AdtDef {
            kind: AdtKind::Enum,
            fields: IndexVec::new(),
            variants: vec![
                VariantDef {
                    name: self.resolver.intern("None"),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: none_fields,
                },
                VariantDef {
                    name: self.resolver.intern("Some"),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: some_fields.clone(),
                },
            ],
            generic_params: vec![self.resolver.intern("T")],
};
        self.register_adt(AdtId::from_raw(1010), option_def.clone());
        self.lang_items.register(LangItem::Option, def_id(1010))
            .expect("builtin lang item registration must not duplicate");
        // Register `Option` by name so the path type `Option<T>` resolves from
        // user code (the lang-item registration alone only ties it to the
        // `Option` lang item, not the name used in type position).
        self.register_adt_with_name(self.resolver.intern("Option"), AdtId::from_raw(1010), option_def);

        // Register `Vec<T>` - ID 1020.
        // An owning collection. We register only its name + generic arity so the
        // frontend resolves `Vec<T>` as a type; the concrete field layout is
        // irrelevant to type resolution (std constructs `Vec` through
        // `Vec::new`/`with_capacity` etc., never via struct literals).
        let vec_t_var = self.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: self.resolver.intern("T"),
        }));
        let mut vec_field_defs = IndexVec::new();
        vec_field_defs.push(FieldDef {
            name: self.resolver.intern("buf"),
            ty: vec_t_var,
        });
        let vec_def = AdtDef {
            kind: AdtKind::Struct,
            fields: vec_field_defs.clone(),
            variants: vec![VariantDef {
                name: self.resolver.intern(""),
                style: crate::adt_def::VariantStyle::Unit,
                fields: vec_field_defs,
            }],
            generic_params: vec![self.resolver.intern("T")],
        };
        self.register_adt(AdtId::from_raw(1020), vec_def.clone());
        self.register_adt_with_name(self.resolver.intern("Vec"), AdtId::from_raw(1020), vec_def);
        // `Vec` is an owning builtin (it owns a heap buffer) — record that it
        // carries a `Drop` impl so `needs_drop` is correct (see `mark_has_drop`).
        self.mark_has_drop(AdtId::from_raw(1020));

        // Register `PhantomData<T>` - ID 1030.
        // A zero-sized marker type. Only its name + arity matter for resolution.
        let pd_t_var = self.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: self.resolver.intern("T"),
        }));
        let mut pd_field_defs = IndexVec::new();
        pd_field_defs.push(FieldDef {
            name: self.resolver.intern("marker"),
            ty: pd_t_var,
        });
        let pd_def = AdtDef {
            kind: AdtKind::Struct,
            fields: pd_field_defs.clone(),
            variants: vec![VariantDef {
                name: self.resolver.intern(""),
                style: crate::adt_def::VariantStyle::Unit,
                fields: pd_field_defs,
            }],
            generic_params: vec![self.resolver.intern("T")],
        };
        self.register_adt(AdtId::from_raw(1030), pd_def.clone());
        self.register_adt_with_name(
            self.resolver.intern("PhantomData"),
            AdtId::from_raw(1030),
            pd_def,
        );

        // Register `Result<T, E>` - ID 1011.
        // The two-variant enum `Ok(T)` / `Err(E)`. Used pervasively by std
        // (e.g. `io::Result<T> = Result<T, io::Error>`). Generic arity 2.
        let mut ok_fields = IndexVec::new();
        ok_fields.push(FieldDef {
            name: self.resolver.intern("0"),
            ty: t_var,
        });
        let mut err_fields = IndexVec::new();
        err_fields.push(FieldDef {
            name: self.resolver.intern("0"),
            ty: t_var,
        });
        let result_def = AdtDef {
            kind: AdtKind::Enum,
            fields: IndexVec::new(),
            variants: vec![
                VariantDef {
                    name: self.resolver.intern("Ok"),
                    style: crate::adt_def::VariantStyle::Unit,
                    fields: ok_fields,
                },
                VariantDef {
                    name: self.resolver.intern("Err"),
                    style: crate::adt_def::VariantStyle::Unit,
                    fields: err_fields,
                },
            ],
            generic_params: vec![self.resolver.intern("T"), self.resolver.intern("E")],
        };
        self.register_adt(AdtId::from_raw(1011), result_def.clone());
        self.register_adt_with_name(
            self.resolver.intern("Result"),
            AdtId::from_raw(1011),
            result_def,
        );
        self.lang_items.register(LangItem::Result, def_id(1011))
            .expect("builtin lang item registration must not duplicate");

        // Register `UnsafeCell<T>` by name (already registered at 1005 above,
        // but `register_adt` alone does not make it name-resolvable in type
        // position).
        self.register_adt_with_name(
            self.resolver.intern("UnsafeCell"),
            AdtId::from_raw(1005),
            AdtDef {
                kind: AdtKind::Struct,
                fields: IndexVec::new(),
                variants: vec![VariantDef {
                    name: self.resolver.intern(""),
                    style: crate::adt_def::VariantStyle::Unit,
                    fields: IndexVec::new(),
                }],
                generic_params: vec![self.resolver.intern("T")],
            },
        );

        // Register `Box<T>` - ID 1040.
        // An owning pointer. Only name + arity matter for resolution; no
        // struct literal is written for `Box` in std.
        let box_t_var = self.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: self.resolver.intern("T"),
        }));
        let mut box_field_defs = IndexVec::new();
        box_field_defs.push(FieldDef {
            name: self.resolver.intern("ptr"),
            ty: box_t_var,
        });
        let box_def = AdtDef {
            kind: AdtKind::Struct,
            fields: box_field_defs.clone(),
            variants: vec![VariantDef {
                name: self.resolver.intern(""),
                style: crate::adt_def::VariantStyle::Unit,
                fields: box_field_defs,
            }],
            generic_params: vec![self.resolver.intern("T")],
        };
        self.register_adt(AdtId::from_raw(1040), box_def.clone());
        self.register_adt_with_name(self.resolver.intern("Box"), AdtId::from_raw(1040), box_def);
        self.mark_has_drop(AdtId::from_raw(1040));

        // Register `String` as a builtin ADT (id 1050). The stdlib treats
        // `String` as a builtin (no `struct String` definition); it exposes
        // `as_ptr`/`len`/`capacity`/`push`/`to_string`/`as_mut_ptr`/`set_len`.
        // We register only name + arity so type resolution works; the concrete
        // layout is irrelevant to type checking.
        let str_t_var = self.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: self.resolver.intern("T"),
        }));
        let mut str_field_defs = IndexVec::new();
        str_field_defs.push(FieldDef {
            name: self.resolver.intern("buf"),
            ty: str_t_var,
        });
        let string_def = AdtDef {
            kind: AdtKind::Struct,
            fields: str_field_defs.clone(),
            variants: vec![VariantDef {
                name: self.resolver.intern(""),
                style: crate::adt_def::VariantStyle::Unit,
                fields: str_field_defs,
            }],
            generic_params: vec![self.resolver.intern("T")],
        };
        self.register_adt(AdtId::from_raw(1050), string_def.clone());
        self.register_adt_with_name(
            self.resolver.intern("String"),
            AdtId::from_raw(1050),
            string_def,
        );
        self.mark_has_drop(AdtId::from_raw(1050));

        // Populate the builtin inherent-method table (Vec/String/Result/Option
        // methods the stdlib calls but which have no user `impl` block).
        self.register_builtin_methods();

        // Register builtin/lang TRAITS by name so `impl Future for X`,
        // `T: Clone`, `impl Drop for Y`, `T: Deref`, etc. resolve. The
        // `LangItem` enum already declares `Future`/`Drop`/`Deref`/`Clone`/…
        // but nothing ever registered them, so every builtin-trait reference
        // was an "unresolved trait" before this. Trait ids are fixed and
        // disjoint from ADT ids (ADTs use 1000–1040; traits start at 2001).
        let mut reg_trait = |id: u32, name: &str| {
            let n = self.resolver.intern(name);
            let tid = glyim_core::def_id::TraitDefId::from_raw(id);
            self.trait_defs.insert(
                tid,
                crate::TraitDef {
                    name: n,
                    methods: Vec::new(),
                    associated_types: Vec::new(),
                },
            );
            self.trait_by_name.insert(n, tid);
        };
        reg_trait(2001, "Future");
        reg_trait(2002, "Drop");
        reg_trait(2003, "Deref");
        reg_trait(2004, "DerefMut");
        reg_trait(2005, "Clone");
        reg_trait(2006, "Copy");
        reg_trait(2007, "Sized");
        reg_trait(2008, "Send");
        reg_trait(2009, "Sync");
        reg_trait(2010, "Unpin");
        reg_trait(2011, "Fn");
        reg_trait(2012, "FnMut");
        reg_trait(2013, "FnOnce");
        reg_trait(2014, "Iterator");
        reg_trait(2015, "IntoIterator");
        reg_trait(2016, "Default");
    }

    /// Populate the builtin inherent-method table (`builtin_method_fns`) for the
    /// collection/enum types the stdlib calls through method syntax but which
    /// have no user `impl` block (`Vec`, `String`, `Result`, `Option`). Method
    /// signatures use `TyKind::Param(i)` to denote the *receiver's* generic
    /// arguments (0 = `T`, 1 = `E` for `Result`/`Option`'s single param); the
    /// call site instantiates them against the receiver's substitution.
    /// stdlib-completion.
    fn register_builtin_methods(&mut self) {
        let t_var = self.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: self.resolver.intern("T"),
        }));
        let _e_var = self.mk_ty(TyKind::Param(ParamTy {
            index: 1,
            name: self.resolver.intern("E"),
        }));
        let usize_ty = self.mk_ty(TyKind::Uint(UintTy::Usize));
        let bool_ty = Ty::BOOL;
        let str_ty = self.mk_ty(TyKind::String);
        let u8_ty = self.mk_ty(TyKind::Uint(UintTy::U8));
        let slice_u8 = self.mk_ty(TyKind::Slice(u8_ty));

        // Precompute every method's output type so the registration closure
        // only mutates the table (avoids holding `&mut self` across repeated
        // `self.mk_ty` calls, which the borrow checker rejects).
        let vec_id = AdtId::from_raw(1020);
        let string_id = AdtId::from_raw(1050);
        let result_id = AdtId::from_raw(1011);
        let option_id = AdtId::from_raw(1010);
        let slice_id = AdtId::from_raw(1060);
        let str_id = AdtId::from_raw(1061);
        let box_id = AdtId::from_raw(1040);

        let vec_as_ptr = self.mk_ty(TyKind::RawPtr(t_var, Mutability::Not));
        let vec_as_mut_ptr = self.mk_ty(TyKind::RawPtr(t_var, Mutability::Mut));
        let u8_as_ptr = self.mk_ty(TyKind::RawPtr(u8_ty, Mutability::Not));
        let u8_as_mut_ptr = self.mk_ty(TyKind::RawPtr(u8_ty, Mutability::Mut));
        let string_subst = self.intern_substitution(vec![GenericArg::Ty(u8_ty)]);
        let string_ty = self.mk_ty(TyKind::Adt(string_id, string_subst));
        let slice_as_ptr = self.mk_ty(TyKind::RawPtr(t_var, Mutability::Not));
        let slice_as_mut_ptr = self.mk_ty(TyKind::RawPtr(t_var, Mutability::Mut));
        let option_subst = self.intern_substitution(vec![GenericArg::Ty(t_var)]);
        let option_ty = self.mk_ty(TyKind::Adt(option_id, option_subst));
        // Associated-function output types (no receiver). `Vec<T>` and friends
        // carry the element type as `Param(0)` so the caller can instantiate.
        let vec_subst = self.intern_substitution(vec![GenericArg::Ty(t_var)]);
        let vec_ty = self.mk_ty(TyKind::Adt(vec_id, vec_subst));
        let result_subst = self.intern_substitution(vec![GenericArg::Ty(t_var), GenericArg::Ty(_e_var)]);
        let result_ty = self.mk_ty(TyKind::Adt(result_id, result_subst));
        // `str::from_utf8(&[u8]) -> Result<&str, E>`: the `&str` element is
        // concrete; the error `E` is left as `Param(1)` so the inferred-output
        // fix in `try_builtin_method` turns it into a fresh inference var
        // (constrained by the caller's `.map_err(Error::new(...))`).
        let str_ref_ty = self.mk_ty(TyKind::Ref(Region::Erased, str_ty, Mutability::Not));
        let ref_slice_u8_ty = self.mk_ty(TyKind::Ref(Region::Erased, slice_u8, Mutability::Not));
        let str_from_utf8_subst = self.intern_substitution(vec![
            GenericArg::Ty(str_ref_ty),
            GenericArg::Ty(_e_var),
        ]);
        let str_from_utf8_result = self.mk_ty(TyKind::Adt(result_id, str_from_utf8_subst));
        let str_find_subst = self.intern_substitution(vec![GenericArg::Ty(usize_ty)]);
        let str_find_result = self.mk_ty(TyKind::Adt(option_id, str_find_subst));
        let box_subst = self.intern_substitution(vec![GenericArg::Ty(t_var)]);
        let box_ty = self.mk_ty(TyKind::Adt(box_id, box_subst));

        // (adt_id, method_name, inputs, output) tuples.
        let entries: Vec<(AdtId, &str, Vec<Ty>, Ty)> = vec![
            // Vec<T>
            (vec_id, "len", vec![], usize_ty),
            (vec_id, "capacity", vec![], usize_ty),
            (vec_id, "as_ptr", vec![], vec_as_ptr),
            (vec_id, "as_mut_ptr", vec![], vec_as_mut_ptr),
            (vec_id, "set_len", vec![usize_ty], Ty::UNIT),
            (vec_id, "push", vec![t_var], Ty::UNIT),
            (vec_id, "is_empty", vec![], bool_ty),
            // String (element u8)
            (string_id, "len", vec![], usize_ty),
            (string_id, "capacity", vec![], usize_ty),
            (string_id, "as_ptr", vec![], u8_as_ptr),
            (string_id, "as_mut_ptr", vec![], u8_as_mut_ptr),
            (string_id, "set_len", vec![usize_ty], Ty::UNIT),
            (string_id, "push", vec![u8_ty], Ty::UNIT),
            (string_id, "to_string", vec![], string_ty),
            // Result<T, E>
            (result_id, "is_ok", vec![], bool_ty),
            (result_id, "is_err", vec![], bool_ty),
            (result_id, "unwrap_or", vec![t_var], t_var),
            (result_id, "unwrap_or_default", vec![], t_var),
            (result_id, "expect", vec![str_ty], t_var),
            // Option<T>
            (option_id, "is_some", vec![], bool_ty),
            (option_id, "is_none", vec![], bool_ty),
            (option_id, "unwrap_or", vec![t_var], t_var),
            (option_id, "unwrap_or_default", vec![], t_var),
            (option_id, "expect", vec![str_ty], t_var),
            (option_id, "take", vec![], option_ty),
            // Slice [T]
            (slice_id, "len", vec![], usize_ty),
            (slice_id, "as_ptr", vec![], slice_as_ptr),
            (slice_id, "as_mut_ptr", vec![], slice_as_mut_ptr),
            (slice_id, "is_empty", vec![], bool_ty),
            // str
            (str_id, "len", vec![], usize_ty),
            (str_id, "is_empty", vec![], bool_ty),
            (str_id, "as_ptr", vec![], u8_as_ptr),
            (str_id, "as_mut_ptr", vec![], u8_as_mut_ptr),
            (str_id, "as_bytes", vec![], self.mk_ty(TyKind::Ref(Region::Erased, slice_u8, Mutability::Not))),
            (str_id, "contains", vec![u8_ty], bool_ty),
            (str_id, "to_string", vec![], string_ty),
            (str_id, "from_utf8", vec![ref_slice_u8_ty], str_from_utf8_result),
            (str_id, "find", vec![str_ty], str_find_result),
            // Inherent associated functions (path-style `Vec::new()`, called
            // through `check_path`'s `Adt::fn` branch, not `recv.method()`).
            (vec_id, "new", vec![], vec_ty),
            (vec_id, "with_capacity", vec![usize_ty], vec_ty),
            (vec_id, "from", vec![], vec_ty),
            (vec_id, "from_raw_parts", vec![], vec_ty),
            (string_id, "new", vec![], string_ty),
            (string_id, "with_capacity", vec![usize_ty], string_ty),
            (string_id, "from", vec![], string_ty),
            (string_id, "from_utf8", vec![], result_ty),
            (string_id, "from_utf8_lossy", vec![ref_slice_u8_ty], string_ty),
            (string_id, "from_str", vec![], result_ty),
            (result_id, "unwrap", vec![], t_var),
            (result_id, "unwrap_or_else", vec![], t_var),
            (result_id, "map", vec![], result_ty),
            (result_id, "map_or", vec![t_var, t_var], t_var),
            (option_id, "unwrap", vec![], t_var),
            (option_id, "unwrap_or_else", vec![], t_var),
            (option_id, "map", vec![], option_ty),
            // Box<T>
            (box_id, "new", vec![t_var], box_ty),
            (box_id, "into_raw", vec![box_ty], self.mk_ty(TyKind::RawPtr(t_var, Mutability::Mut))),
            (box_id, "from_raw", vec![self.mk_ty(TyKind::RawPtr(t_var, Mutability::Mut))], box_ty),
            (box_id, "leak", vec![box_ty], self.mk_ty(TyKind::Ref(Region::Erased, t_var, Mutability::Mut))),
            (box_id, "as_ptr", vec![], self.mk_ty(TyKind::RawPtr(t_var, Mutability::Not))),
        ];

        for (adt_id, name, inputs, output) in entries {
            let fn_id = FnDefId::from_raw(self.next_builtin_fn_id);
            self.next_builtin_fn_id += 1;
            let n = self.resolver.intern(name);
            let inputs_subst = self.intern_substitution(
                inputs.iter().map(|t| GenericArg::Ty(*t)).collect(),
            );
            self.builtin_method_fns.insert(
                (adt_id, n),
                (
                    fn_id,
                    FnSig {
                        inputs: inputs_subst,
                        output,
                        c_variadic: false,
                        unsafety: glyim_core::primitives::Safety::Safe,
                        abi: glyim_core::primitives::Abi::Glyim,
                    },
                ),
            );
        }
    }

    /// Look up a builtin inherent method by receiver ADT id + method name.
    /// Returns the synthetic `FnDefId` and the method's `FnSig` template
    /// (generic params as `TyKind::Param` referring to the receiver's generic
    /// args). The caller instantiates the output type against the receiver's
    /// substitution. Returns `None` when no builtin method matches.
    pub fn lookup_builtin_method(&self, adt_id: AdtId, name: Name) -> Option<(FnDefId, FnSig)> {
        self.builtin_method_fns.get(&(adt_id, name)).map(|(id, sig)| (*id, sig.clone()))
    }
}

impl TypeLookup for TyCtxMut {
    fn ty_kind(&self, ty: Ty) -> &TyKind {
        self.ty_kind(ty)
    }
    fn ty_flags(&self, ty: Ty) -> TypeFlags {
        self.ty_flags(ty)
    }
    fn substitution_args(&self, sub: Substitution) -> &[GenericArg] {
        self.substitution_args(sub)
    }
    fn name_str(&self, name: Name) -> &str {
        self.resolver.resolve(name)
    }
    fn error_ty(&self) -> Ty {
        Ty::ERROR
    }
    fn is_interior_mutable_adt(&self, adt_id: AdtId) -> bool {
        self.interior_mutable_adt_ids.contains(&adt_id)
    }
    fn adt_def(&self, adt_id: AdtId) -> Option<&AdtDef> {
        self.adt_defs.get(&adt_id)
    }
    fn field_ty(&self, adt_id: AdtId, field_idx: usize) -> Ty {
        if let Some(def) = self.adt_defs.get(&adt_id) {
            return def
                .fields
                .as_slice()
                .get(field_idx)
                .map(|f| f.ty)
                .unwrap_or_else(|| self.error_ty());
        }
        if let Some(repr) = self.adt_reprs.get(&adt_id) {
            return repr
                .field_tys
                .get(field_idx)
                .copied()
                .unwrap_or_else(|| self.error_ty());
        }
        self.error_ty()
    }
}

#[cfg(test)]
mod interior_mutability_tests {
    use super::*;
    use glyim_core::interner::Interner;

    #[test]
    fn test_unsafe_cell_is_interior_mutable() {
        let ctx_mut = TyCtxMut::new(Interner::new());
        // register_builtin_ranges is called in new, so UnsafeCell (1005) is registered.
        let ctx = ctx_mut.freeze();
        let unsafe_cell_adt = AdtId::from_raw(1005);
        assert!(ctx.is_interior_mutable_adt(unsafe_cell_adt));
    }

    #[test]
    fn test_cell_is_interior_mutable() {
        // Cell<T> contains UnsafeCell<T> internally.
        let mut ctx_mut = TyCtxMut::new(Interner::new());
        let t_var = ctx_mut.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: ctx_mut.resolver.intern("T"),
        }));
        // Build substitution for UnsafeCell<T>
        let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(t_var)]);
        let unsafe_cell_ty = ctx_mut.mk_ty(TyKind::Adt(AdtId::from_raw(1005), subst));
        let cell_def = {
            let mut field_defs = IndexVec::new();
            field_defs.push(FieldDef {
                name: ctx_mut.resolver.intern("value"),
                ty: unsafe_cell_ty,
            });
            let field_defs_clone = field_defs.clone();
            AdtDef {
                kind: AdtKind::Struct,
                fields: field_defs,
                variants: vec![VariantDef {
                    name: ctx_mut.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: field_defs_clone,
                }],
                generic_params: vec![],
}
        };
        ctx_mut.register_adt(AdtId::from_raw(1006), cell_def);
        let ctx = ctx_mut.freeze();
        let cell_adt = AdtId::from_raw(1006);
        assert!(ctx.is_interior_mutable_adt(cell_adt));
    }

    #[test]
    fn test_option_builtin_registered() {
        // `Option<T>` is registered as a two-variant enum at id 1010 by
        // register_builtin_ranges (called in `new`).
        let mut ctx_mut = TyCtxMut::new(Interner::new());
        // Build Option<T> type to confirm the ADT exists and is an enum.
        let t_var = ctx_mut.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: ctx_mut.resolver.intern("T"),
        }));
        let opt_subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(t_var)]);
        let option_ty = ctx_mut.mk_ty(TyKind::Adt(AdtId::from_raw(1010), opt_subst));
        let ctx = ctx_mut.freeze();
        match ctx.ty_kind(option_ty) {
            TyKind::Adt(adt_id, _) => {
                assert_eq!(*adt_id, AdtId::from_raw(1010));
                let def = ctx.adt_def(*adt_id).expect("Option ADT should be registered");
                assert_eq!(def.variants.len(), 2, "Option has None + Some variants");
                assert_eq!(ctx.name_str(def.variants[0].name), "None");
                assert_eq!(ctx.name_str(def.variants[1].name), "Some");
            }
            other => panic!("expected Option ADT, got {other:?}"),
        }
    }

    #[test]
    fn test_plain_struct_not_interior_mutable() {
        let mut ctx_mut = TyCtxMut::new(Interner::new());
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
        let plain_def = {
            let mut field_defs = IndexVec::new();
            field_defs.push(FieldDef {
                name: ctx_mut.resolver.intern("x"),
                ty: i32_ty,
            });
            let field_defs_clone = field_defs.clone();
            AdtDef {
                kind: AdtKind::Struct,
                fields: field_defs,
                variants: vec![VariantDef {
                    name: ctx_mut.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: field_defs_clone,
                }],
                generic_params: vec![],
}
        };
        ctx_mut.register_adt(AdtId::from_raw(1007), plain_def);
        let ctx = ctx_mut.freeze();
        let plain_adt = AdtId::from_raw(1007);
        assert!(!ctx.is_interior_mutable_adt(plain_adt));
    }

    #[test]
    fn test_ref_cell_is_interior_mutable() {
        // RefCell<T> contains UnsafeCell<T> internally.
        let mut ctx_mut = TyCtxMut::new(Interner::new());
        let t_var = ctx_mut.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: ctx_mut.resolver.intern("T"),
        }));
        let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(t_var)]);
        let unsafe_cell_ty = ctx_mut.mk_ty(TyKind::Adt(AdtId::from_raw(1005), subst));
        let ref_cell_def = {
            let mut field_defs = IndexVec::new();
            field_defs.push(FieldDef {
                name: ctx_mut.resolver.intern("value"),
                ty: unsafe_cell_ty,
            });
            let field_defs_clone = field_defs.clone();
            AdtDef {
                kind: AdtKind::Struct,
                fields: field_defs,
                variants: vec![VariantDef {
                    name: ctx_mut.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: field_defs_clone,
                }],
                generic_params: vec![],
}
        };
        ctx_mut.register_adt(AdtId::from_raw(1008), ref_cell_def);
        let ctx = ctx_mut.freeze();
        let ref_cell_adt = AdtId::from_raw(1008);
        assert!(ctx.is_interior_mutable_adt(ref_cell_adt));
    }

    #[test]
    fn test_cache_invalidation_after_registering_cell_field() {
        // Regression for §1.2: the interior-mutability cache must not return a
        // stale `false` for an ADT `A` that transitively contains a `Cell`-like
        // ADT `B`, when `B` is registered *after* `A` was first queried.
        let mut ctx_mut = TyCtxMut::new(Interner::new());
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));

        // `B` holds a single `i32` (no interior mutability). Register it first.
        let b_def = {
            let mut field_defs = IndexVec::new();
            field_defs.push(FieldDef {
                name: ctx_mut.resolver.intern("x"),
                ty: i32_ty,
            });
            let field_defs_clone = field_defs.clone();
            AdtDef {
                kind: AdtKind::Struct,
                fields: field_defs,
                variants: vec![VariantDef {
                    name: ctx_mut.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: field_defs_clone,
                }],
                generic_params: vec![],
}
        };
        ctx_mut.register_adt(AdtId::from_raw(2000), b_def);

        // `A` contains `B`. Register it and freeze. A's interior mutability is
        // `false` because `B` is plain here.
        let b_subst = ctx_mut.intern_substitution(vec![]);
        let b_ty = ctx_mut.mk_ty(TyKind::Adt(AdtId::from_raw(2000), b_subst));
        let a_def = {
            let mut field_defs = IndexVec::new();
            field_defs.push(FieldDef {
                name: ctx_mut.resolver.intern("b"),
                ty: b_ty,
            });
            let field_defs_clone = field_defs.clone();
            AdtDef {
                kind: AdtKind::Struct,
                fields: field_defs,
                variants: vec![VariantDef {
                    name: ctx_mut.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: field_defs_clone,
                }],
                generic_params: vec![],
}
        };
        ctx_mut.register_adt(AdtId::from_raw(2001), a_def);
        let ctx = ctx_mut.freeze();
        let a_adt = AdtId::from_raw(2001);
        assert!(!ctx.is_interior_mutable_adt(a_adt), "A transitively contains plain B -> false");

        // Now re-open the context and make `B` contain `UnsafeCell` (1005), which
        // is interior-mutable. This must invalidate the cached `false` for A.
        let mut ctx_mut2 = TyCtxMut::new(Interner::new());
        // Re-register builtins + B + A the same way, but with B now holding UnsafeCell.
        let t_var = ctx_mut2.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: ctx_mut2.resolver.intern("T"),
        }));
        let unsafe_cell_subst = ctx_mut2.intern_substitution(vec![GenericArg::Ty(t_var)]);
        let unsafe_cell_ty = ctx_mut2.mk_ty(TyKind::Adt(AdtId::from_raw(1005), unsafe_cell_subst));
        let b2_def = {
            let mut field_defs = IndexVec::new();
            field_defs.push(FieldDef {
                name: ctx_mut2.resolver.intern("cell"),
                ty: unsafe_cell_ty,
            });
            let field_defs_clone = field_defs.clone();
            AdtDef {
                kind: AdtKind::Struct,
                fields: field_defs,
                variants: vec![VariantDef {
                    name: ctx_mut2.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: field_defs_clone,
                }],
                generic_params: vec![],
}
        };
        ctx_mut2.register_adt(AdtId::from_raw(2000), b2_def);
        let b2_subst = ctx_mut2.intern_substitution(vec![]);
        let b2_ty = ctx_mut2.mk_ty(TyKind::Adt(AdtId::from_raw(2000), b2_subst));
        let a2_def = {
            let mut field_defs = IndexVec::new();
            field_defs.push(FieldDef {
                name: ctx_mut2.resolver.intern("b"),
                ty: b2_ty,
            });
            let field_defs_clone = field_defs.clone();
            AdtDef {
                kind: AdtKind::Struct,
                fields: field_defs,
                variants: vec![VariantDef {
                    name: ctx_mut2.resolver.intern(""),
    style: crate::adt_def::VariantStyle::Unit,
                    fields: field_defs_clone,
                }],
                generic_params: vec![],
}
        };
        ctx_mut2.register_adt(AdtId::from_raw(2001), a2_def);
        let ctx2 = ctx_mut2.freeze();
        assert!(ctx2.is_interior_mutable_adt(a_adt), "after B gains an UnsafeCell, A must recompute true");
        assert!(ctx2.is_interior_mutable_adt(AdtId::from_raw(2000)));
    }
}

#[cfg(test)]
mod subst_ty_tests {
    use super::*;
    use glyim_core::interner::Interner;
    use glyim_core::primitives::{IntTy, UintTy};
    use crate::const_val::{Const, ConstKind, ParamConst};
    use std::collections::HashMap;

    /// Phase 2 (GLYIM_DESTUB_PLAN): `subst_ty` must substitute the array
    /// element type. A `[T; 3]` with `T = Param(0)` substituted by `{0: i32}`
    /// must yield `[i32; 3]` (previously the `_ => ty` fallback returned the
    /// array unchanged, leaving `Param(0)` as the element).
    #[test]
    fn subst_ty_substitutes_array_element() {
        let mut tcx = TyCtxMut::new(Interner::new());
        let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
        let usize_ty = tcx.mk_ty(TyKind::Uint(UintTy::Usize));
        let param0 = tcx.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: tcx.resolver.intern("T"),
        }));
        let len = Const {
            kind: ConstKind::Int(3),
            ty: usize_ty,
        };
        let arr = tcx.mk_ty(TyKind::Array(param0, len));

        let subst = HashMap::from([(0u32, GenericArg::Ty(i32_ty))]);
        let result = tcx.subst_ty(arr, &subst);

        match tcx.ty_kind(result) {
            TyKind::Array(inner, _) => {
                assert_eq!(*inner, i32_ty, "array element must be substituted to i32");
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn subst_ty_substitutes_const_param_array_length() {
        let mut tcx = TyCtxMut::new(Interner::new());
        let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
        let usize_ty = tcx.mk_ty(TyKind::Uint(UintTy::Usize));
        let elem_param = tcx.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: tcx.resolver.intern("T"),
        }));
        // `[T; N]` where `N` is a const parameter (index 1).
        let len_param = Const {
            kind: ConstKind::Param(ParamConst {
                index: 1,
                name: tcx.resolver.intern("N"),
            }),
            ty: usize_ty,
        };
        let arr = tcx.mk_ty(TyKind::Array(elem_param, len_param));

        let concrete_len = Const {
            kind: ConstKind::Int(5),
            ty: usize_ty,
        };
        let subst = HashMap::from([
            (0u32, GenericArg::Ty(i32_ty)),
            (
                1u32,
                GenericArg::Const(concrete_len.clone()),
            ),
        ]);
        let result = tcx.subst_ty(arr, &subst);

        match tcx.ty_kind(result) {
            TyKind::Array(inner, len) => {
                assert_eq!(*inner, i32_ty, "array element must be substituted to i32");
                assert_eq!(
                    len.kind,
                    ConstKind::Int(5),
                    "const-parameter length must be substituted to 5"
                );
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }
}
