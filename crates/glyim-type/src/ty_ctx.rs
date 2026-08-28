use crate::adt_def::*;
use crate::auto_trait::*;
use crate::display::TypeLookup;
use crate::type_arena::TypeArena;
use crate::ty_ctx_mut::TyCtxMut;
use crate::flags::*;
use crate::fn_sig::FnSig;
use crate::lang_items::LangItems;
use crate::region::*;
use crate::substitution::*;
use crate::ty::*;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{AdtId, ClosureId, ConstDefId, FnDefId, LocalDefId, OpaqueTyId, TraitDefId};
use glyim_core::interner::{Interner, Name};
use glyim_core::primitives::Mutability;
use std::collections::{HashMap, HashSet};

/// TyCtx.
pub struct TyCtx {
    /// Shared, canonical type table for this compilation (see `type_arena`).
    /// A `&'static` so handles allocated by any derived `TyCtxMut` are valid
    /// here — the fix for the cross-context handle-validity ("aliasing") class.
    pub(crate) arena: &'static TypeArena,
    pub(crate) regions: IndexVec<RegionVid, Region>,
    pub(crate) resolver: Interner,
    pub(crate) auto_trait_registry: AutoTraitRegistry,
    pub(crate) deref_registry: crate::deref::DerefRegistry,
    pub(crate) adt_reprs: HashMap<AdtId, AdtRepr>,
    /// Concrete hidden types for opaque types (`impl Trait` / `type X = impl
    /// Trait`). Populated at the opaque type's defining use (by typeck) so that
    /// auto-trait computation (§7.2) can recurse into the underlying type
    /// instead of assuming zero auto traits.
    pub(crate) opaque_hidden: HashMap<OpaqueTyId, Ty>,
    pub(crate) interior_mutable_adt_ids: HashSet<AdtId>,
/// Struct.
    pub adt_defs: HashMap<AdtId, AdtDef>,
    /// Maps an ADT's (HIR-interned) name to its `AdtId`. Populated by
    /// `register_adt_with_name` during typeck so ADT type resolution by name
    /// works on the frozen context too (needed by `PipelineLowerCtx::
    /// iterator_next_fn`, Phase 1 GLYIM_DESTUB_PLAN).
    pub(crate) adt_by_name: HashMap<Name, AdtId>,
    pub(crate) trait_defs: HashMap<glyim_core::def_id::TraitDefId, crate::TraitDef>,
    pub(crate) variant_types: HashMap<AdtId, Vec<Ty>>,
    pub(crate) fn_sigs: HashMap<FnDefId, FnSig>,
    pub(crate) const_tys: HashMap<ConstDefId, Ty>,
    pub(crate) closure_sigs: HashMap<ClosureId, FnSig>,
    /// `ClosureId` → synthetic `AdtId` for the closure's captured environment
    /// (populated by `TyCtxMut::register_closure`). Lets the debug-info pass
    /// recover per-capture member types from a `TyKind::Closure`.
    pub(crate) closure_adt_map: HashMap<ClosureId, AdtId>,
    /// Concrete associated-type projection table (see `TyCtxMut::impl_assoc_types`).
    pub(crate) impl_assoc_types:
        HashMap<(Ty, glyim_core::def_id::TraitDefId), Vec<(Name, Ty)>>,
    pub(crate) body_tys: HashMap<LocalDefId, Ty>,
    pub(crate) lang_items: LangItems,
    /// `AdtId`s that have an explicit `Drop` impl (or are owning builtins such as
    /// `String`/`Vec`/`Box`). Consulted by `needs_drop` so that a type whose
    /// *fields* are all primitives (e.g. `String` backed by a raw pointer) is
    /// still correctly reported as needing drop glue. Populated via
    /// `TyCtxMut::mark_has_drop`.
    pub(crate) drop_impls: HashSet<AdtId>,
    /// Concrete trait-method dispatch table, populated during typeck from
    /// `impl Trait for Type` items: maps `(trait_def_id, Self-AdtId)` to the
    /// method name → `FnDefId` of that impl's method. Lets monomorphization
    /// and the MIR interpreter devirtualize a generic-bound call
    /// (`f.poll()` where `f: F: Future`) once the receiver's concrete type is
    /// known, without re-scanning the HIR.
    pub(crate) impl_method_fns:
        HashMap<(glyim_core::def_id::TraitDefId, AdtId), HashMap<Name, FnDefId>>,
}

impl TyCtx {
/// to_mut.
    /// Produce a fresh `TyCtxMut` that **shares the same canonical type arena**
    /// as this frozen context. Any type/substitution allocated through the
    /// returned mutator is valid when read back through this (or any other)
    /// view of the compilation — this is what makes drop-glue elaboration able
    /// to synthesize flag-array types without breaking the consumer's handles
    /// (the P0 canonical-interner fix). Unlike the old deep-`Vec`-clone
    /// `to_mut`, this copies only the `&'static` arena pointer, so it is cheap
    /// and cannot desynchronize the tables.
    pub fn to_mut(&self) -> TyCtxMut {
        TyCtxMut::from_ty_ctx(self)
    }

/// ty_kind.
    pub fn ty_kind(&self, ty: Ty) -> &TyKind {
        self.arena.ty_kind(ty)
    }

    /// Access the language-item registry (builtin `Option`/`Range`/`Drop`/…).
    pub fn lang_items(&self) -> &LangItems {
        &self.lang_items
    }

/// ty_flags.
    pub fn ty_flags(&self, ty: Ty) -> TypeFlags {
        self.arena.ty_flags(ty)
    }

/// substitution_args.
    pub fn substitution_args(&self, sub: Substitution) -> &[GenericArg] {
        self.arena.substitution_args(sub)
    }

    /// Intern a type into the shared (per-compilation) arena. Safe on a frozen
    /// `&TyCtx` because the arena is `&'static` and interning is the only
    /// mutation — under the single-threaded-per-compilation invariant this is
    /// sound (same design as the P0 canonical interner). Needed by lowering
    /// passes that synthesize types while only holding a `&TyCtx`
    /// (e.g. `PipelineLowerCtx::iterator_next_fn`, Phase 1 GLYIM_DESTUB_PLAN).
    pub fn mk_ty(&self, kind: TyKind) -> Ty {
        let flags = crate::flags::compute_flags(&kind, self, 0);
        self.arena.alloc_ty(kind, flags)
    }

    /// Build a reference type, interning into the shared arena (see `mk_ty`).
    pub fn mk_ref(&self, region: Region, ty: Ty, mutability: Mutability) -> Ty {
        self.mk_ty(TyKind::Ref(region, ty, mutability))
    }

    /// Intern a substitution into the shared arena (see `mk_ty`).
    pub fn intern_substitution(&self, args: Vec<GenericArg>) -> Substitution {
        self.arena.intern_substitution(args)
    }

    /// Resolve a registered ADT by its (HIR-interned) name. Populated by
    /// `register_adt_with_name` during typeck and carried into the frozen
    /// context, so lowering passes holding only a `&TyCtx` can map a type name
    /// to its `AdtId` (Phase 1 GLYIM_DESTUB_PLAN).
    pub fn adt_id_by_name(&self, name: Name) -> Option<AdtId> {
        self.adt_by_name.get(&name).copied()
    }

/// region.
    pub fn region(&self, vid: RegionVid) -> &Region {
        &self.regions[vid]
    }

/// resolver.
    pub fn resolver(&self) -> &Interner {
        &self.resolver
    }

/// name_str.
    pub fn name_str(&self, name: Name) -> &str {
        self.resolver.resolve(name)
    }

    /// Resolve an associated-type projection `Self::Item` / `Type::Item` to its
    /// defining type via the projection table populated at impl-registration
    /// time (plan unstub-5 P5). Returns `None` when no matching impl exists.
    pub fn resolve_associated_type(
        &self,
        self_ty: Ty,
        trait_def_id: TraitDefId,
        assoc_name: Name,
    ) -> Option<Ty> {
        // Fast path: exact (self_ty handle, trait) match. `Ty` equality is by
        // interned raw index, so two handles for the same logical type (e.g.
        // the same `Adt` reached via different code paths) may have different
        // raw indices and miss here.
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
        // Fallback: match by the underlying `AdtId` (the by_self_ty semantics).
        // An impl's associated type is keyed by the self type's ADT, not a
        // specific `Ty` handle, so this resolves projections whose `self`
        // carries a different raw index than the one used at impl registration
        // (e.g. generic `F::Output` after the call's `substs` substitution at
        // codegen — the M5 single-await `block_on` resolution gap).
        let by_adt = match self.ty_kind(self_ty) {
            TyKind::Adt(adt_id, _) => {
                let self_adt = *adt_id;
                self.impl_assoc_types
                    .iter()
                    .find(|((sty, trait_id), entries)| {
                        *trait_id == trait_def_id
                            && matches!(self.ty_kind(*sty), TyKind::Adt(b, _) if b == &self_adt)
                            && entries.iter().any(|(n, _)| *n == assoc_name)
                    })
                    .and_then(|((_, _), entries)| {
                        entries
                            .iter()
                            .find(|(name, _)| *name == assoc_name)
                            .map(|(_, ty)| *ty)
                    })
            }
            _ => None,
        };
        if by_adt.is_some() {
            return by_adt;
        }
        // Final fallback: the projection's `self_ty` may still be an unresolved
        // `Param` at codegen (the call-site substitution did not inline the
        // concrete `Self` into the projection's inner `trait_ref.substs`). When
        // no self-directed entry matches, resolve by `(trait_def_id, assoc_name)`
        // alone — correct for the common single-impl case (e.g. `Future::Output`)
        // and only reached when both self-directed lookups missed. If multiple
        // distinct impls could satisfy the trait+name, we conservatively refuse
        // (return `None`) rather than guess.
        let mut found: Option<Ty> = None;
        let mut ambiguous = false;
        for ((_, tid), entries) in self.impl_assoc_types.iter() {
            if *tid != trait_def_id {
                continue;
            }
            for (n, ty) in entries.iter() {
                if *n == assoc_name
                    && found != Some(*ty) {
                        if found.is_some() {
                            ambiguous = true;
                            break;
                        }
                        found = Some(*ty);
                    }
            }
            if ambiguous {
                break;
            }
        }
        if ambiguous {
            None
        } else {
            found
        }
    }

    /// Resolve an associated-type projection `Self::Item` / `Type::Item` to its
    /// defining type via the projection table populated at impl-registration
    /// time (plan unstub-5 P5). Returns `None` when no matching impl exists.
    /// Unlike `resolve_associated_type`, this reverse lookup finds the entry by
    /// `self_ty` + assoc name alone (the bug-aware variant used when the trait
    /// is not yet known at the call site) — used for concrete `Type::Item` paths.
    /// Like [`Self::resolve_associated_type_by_self_ty`] but keys on the
    /// associated-type *name string* rather than a `Name` handle. This is
    /// robust against cross-arena `Name` handle mismatches (the codegen
    /// `TyCtx` and the typeck `TyCtxMut` may intern the same identifier into
    /// different `Name` indices), which would otherwise make a handle-based
    /// lookup miss a perfectly valid impl.
    pub fn resolve_associated_type_by_self_ty_name(
        &self,
        self_ty: Ty,
        assoc_name: &str,
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
                sty_matches
                    && entries
                        .iter()
                        .any(|(n, _)| self.name_str(*n) == assoc_name)
            })
            .and_then(|((_, _), entries)| {
                entries
                    .iter()
                    .find(|(name, _)| self.name_str(*name) == assoc_name)
                    .map(|(_, ty)| *ty)
            })
    }


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

    /// Whether `ty` is `Sized` (has a known, non-zero/complete layout).
    ///
    /// Everything is sized except the genuinely unsized kinds: `str`, slice `[T]`,
    /// `dyn Trait` (trait objects), and ADTs whose last field is itself unsized
    /// (recursively). This is the structural `Sized` check used by the trait
    /// solver for `T: Sized` bounds (de-stubbing plan §8.1).
    pub fn is_sized(&self, ty: Ty) -> bool {
        match self.ty_kind(ty) {
            TyKind::Slice(_) | TyKind::Dynamic(..) | TyKind::Opaque(..) => false,
            TyKind::Array(inner, _) => self.is_sized(*inner),
            TyKind::Tuple(substs) => {
                self.substitution_args(*substs)
                    .iter()
                    .all(|arg| match arg {
                        GenericArg::Ty(t) => self.is_sized(*t),
                        _ => true,
                    })
            }
            TyKind::Adt(adt_id, _) => {
                if let Some(adt_def) = self.adt_def(*adt_id) {
                    // An ADT is sized unless it has a field that is unsized
                    // (the lang-level rule for the by-value-last-field DST).
                    adt_def.variants.iter().all(|v| {
                        v.fields
                            .iter()
                            .all(|f| self.is_sized(f.ty))
                    })
                } else {
                    // Unknown/unregistered ADT: conservatively treat as
                    // *un*sized so generic `T: Sized` bounds and codegen do not
                    // silently accept a type whose layout we cannot know.
                    // Surface in debug builds so a genuinely sized-but-unregistered
                    // ADT is not silently mishandled.
                    debug_assert!(
                        false,
                        "is_sized: unknown/unregistered ADT {adt_id:?}; assuming UNSIZED"
                    );
                    false
                }
            }
            _ => true,
        }
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

/// ty_is_error.
    pub fn ty_is_error(&self, ty: Ty) -> bool {
        self.ty_flags(ty).contains(TypeFlags::HAS_ERROR)
    }

/// ty_has_depth_overflow.
    pub fn ty_has_depth_overflow(&self, ty: Ty) -> bool {
        self.ty_flags(ty).contains(TypeFlags::HAS_DEPTH_OVERFLOW)
    }

/// auto_trait_flags.
    pub fn auto_trait_flags(&self, ty: Ty) -> AutoTraitFlags {
        compute_auto_traits(ty, self, &self.auto_trait_registry, &self.adt_reprs)
    }

/// implements_auto_trait.
    pub fn implements_auto_trait(&self, ty: Ty, auto_trait: AutoTrait) -> bool {
        self.auto_trait_flags(ty).contains(auto_trait.flag())
    }

/// has_negative_impl.
    pub fn has_negative_impl(&self, adt_id: AdtId, auto_trait: AutoTrait) -> bool {
        self.auto_trait_registry
            .has_negative_impl(adt_id, auto_trait)
    }

    /// Mechanically dereference a type for auto-deref in method resolution
    /// (de-stubbing plan §9.1). Handles the structural cases that need no trait
    /// database: shared/mutable references and raw pointers. ADT `Deref` impls
    /// (e.g. `Box<T>`, `Rc<T>`) require the `Deref` trait impl to be registered,
    /// which is gated on the HIR → `TraitContext` population, so they return
    /// `None` here rather than guessing.
    pub fn deref_ty(&self, ty: Ty) -> Option<Ty> {
        match self.ty_kind(ty) {
            TyKind::Ref(_, inner, _) => Some(*inner),
            TyKind::RawPtr(inner, _) => Some(*inner),
            // Phase 5 (GLYIM_DESTUB_PLAN): also consult the Deref impl registry
            // so autoderef can step through user `impl Deref` for ADTs
            // (Box/Rc/Vec/etc.), not just the structural &T / *T cases.
            TyKind::Adt(adt_id, sub) => {
                if let Some(target) = self.deref_registry.exact_target(ty) {
                    return Some(target);
                }
                // Generic `impl<T> Deref for Adt<T> { type Target = T; }`: the
                // most common case has `Target` equal to one of the self
                // parameters, so substitute it positionally. More elaborate
                // target types would require building a fresh `Ty`, which the
                // frozen `TyCtx` cannot do; that is acceptable because real
                // `Deref` impls use `type Target = T`.
                if let Some((_self_params, target)) = self.deref_registry.template(*adt_id)
                    && let TyKind::Param(p) = self.ty_kind(*target)
                        && let Some(arg) = self.substitution_args(*sub).get(p.index as usize)
                            && let GenericArg::Ty(t) = arg {
                                return Some(*t);
                            }
                None
            }
            _ => None,
        }
    }

    /// Phase 5 (GLYIM_DESTUB_PLAN): test/introspection helper returning the
    /// registered `Deref::Target` type for a `Deref`-implementing ADT, as
    /// recorded by `populate_deref_registry` during typeck. Returns `None` if
    /// no `impl Deref` for `adt_id` was registered.
    pub fn deref_registry_target_for(&self, adt_id: AdtId) -> Option<Ty> {
        self.deref_registry
            .template(adt_id)
            .map(|(_self_params, target)| *target)
    }

/// has_manual_impl.
    pub fn has_manual_impl(&self, adt_id: AdtId, auto_trait: AutoTrait) -> bool {
        self.auto_trait_registry.has_manual_impl(adt_id, auto_trait)
    }

/// adt_repr.
    pub fn adt_repr(&self, adt_id: AdtId) -> Option<&AdtRepr> {
        self.adt_reprs.get(&adt_id)
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

/// adt_def.
    pub fn adt_def(&self, id: AdtId) -> Option<&AdtDef> {
        self.adt_defs.get(&id)
    }

    /// Number of generic type parameters declared on `adt_id`
    /// (`struct S<T, U>` → 2). Returns 0 for non-generic ADTs and for unknown
    /// ADT ids. Drives substitution-arity checking in type resolution
    /// (unstub-5 P1.4).
    pub fn adt_generic_arity(&self, adt_id: AdtId) -> usize {
        self.adt_defs
            .get(&adt_id)
            .map(|d| d.generic_params.len())
            .unwrap_or(0)
    }

    /// Resolve a trait method call on a *concrete* receiver type to the
    /// implementing function's `FnDefId`. Used to devirtualize generic-bound
    /// calls (`f.poll()` where `f: F: Future` becomes a call on the
    /// monomorphized `TwoFuture`) at monomorphization and interpretation time,
    /// without re-scanning the HIR.
    ///
    /// `recv_ty` is the (already-concrete) type of the call's first argument
    /// (the receiver). The receiver may be `Self`, `&Self`, or `&mut Self`, so
    /// all three shapes are probed. Returns `None` if no `impl` of
    /// `trait_def_id` for the receiver's underlying ADT defines `method_name`.
    pub fn resolve_trait_method(
        &self,
        trait_def_id: glyim_core::def_id::TraitDefId,
        recv_ty: Ty,
        method_name: Name,
    ) -> Option<FnDefId> {
        // Peel any leading reference(s) so the impl lookup sees the underlying
        // `Self` ADT. A method call `f.poll()` carries a `&mut Self` receiver,
        // but the `impl Future for TwoFuture` is keyed on `TwoFuture` itself.
        let mut base = recv_ty;
        while let TyKind::Ref(_, inner, _) = self.ty_kind(base) {
            base = *inner;
        }
        let candidates = [
            base,
            self.mk_ref(Region::Erased, base, Mutability::Not),
            self.mk_ref(Region::Erased, base, Mutability::Mut),
        ];
        for cand in candidates {
            if let TyKind::Adt(adt_id, _) = self.ty_kind(cand)
                && let Some(methods) = self.impl_method_fns.get(&(trait_def_id, *adt_id))
                    && let Some(&fn_def_id) = methods.get(&method_name) {
                        return Some(fn_def_id);
                    }
        }
        None
    }

    /// Whether `adt_id` has an explicit `Drop` impl (or is a registered owning
    /// builtin). This is the single authority consulted by `needs_drop`; it
    /// replaces the previous per-crate guesses (`glyim-lower` hardcoded
    /// `String → true`, `glyim-opt` conservative `true` for unknown ADTs) which
    /// could disagree on identical types — the soundness risk flagged by the
    /// de-stubbing plan §8.2/§12.3.
    pub fn has_drop_impl(&self, adt_id: AdtId) -> bool {
        self.drop_impls.contains(&adt_id)
    }

    /// Authoritative `needs_drop` for the whole compiler.
    ///
    /// A type needs drop glue if it is `Copy`-adjacent (never needs drop), is a
    /// reference/raw-pointer (pointee is not owned), is a function pointer or
    /// function definition, or otherwise contains — transitively — a field that
    /// needs drop. Unions need drop (the user is responsible for the active
    /// variant). An `Adt` with a registered `Drop` impl needs drop regardless of
    /// its field types. Recursion is guarded by `visited` so self-referential
    /// types (e.g. `Box<Self>`) terminate; a cycle is treated as "needs drop" if
    /// any reachable field does, which is the correct answer for `Box<Self>`
    /// (the `Box` itself carries a `Drop` impl).
    ///
    /// Types the model cannot inspect (`dyn Trait`, generic parameters,
    /// projections, error/opaque types) return `false`: dropping them is
    /// impossible to do correctly without their concrete layout, and the
    /// alternative (spurious drop glue) is the worse failure mode. This matches
    /// the prior `glyim-lower` behaviour and is the safer default.
    pub fn needs_drop(&self, ty: Ty) -> bool {
        let mut visited = HashSet::new();
        self.needs_drop_rec(ty, &mut visited)
    }

    fn needs_drop_rec(&self, ty: Ty, visited: &mut HashSet<Ty>) -> bool {
        if self.is_copy(ty) {
            return false;
        }
        if !visited.insert(ty) {
            // Already exploring this type along the current path: a cycle.
            // Conservative `false` here; an enclosing field check may still
            // return `true` via an outer `Drop` impl or a non-cyclic field.
            return false;
        }
        let result = match self.ty_kind(ty) {
            TyKind::Ref(_, _, _) | TyKind::RawPtr(_, _) => false,
            // `String` is an owning builtin: it carries a destructor even though
            // it is represented as a standalone `TyKind` (no embedded `AdtId`),
            // so it must needs-drop. Mirrors the plan §8.2 intent of keying
            // drop-carrying-ness off the `Drop` lang item for owning types.
            TyKind::String => true,
            TyKind::FnPtr(_) | TyKind::FnDef(_, _) => false,
            TyKind::Slice(inner) | TyKind::Array(inner, _) => self.needs_drop_rec(*inner, visited),
            TyKind::Tuple(substs) => self
                .substitution_args(*substs)
                .iter()
                .any(|a| matches!(a, GenericArg::Ty(t) if self.needs_drop_rec(*t, visited))),
            TyKind::Closure(_, substs) => self
                .substitution_args(*substs)
                .iter()
                .any(|a| matches!(a, GenericArg::Ty(t) if self.needs_drop_rec(*t, visited))),
            TyKind::Adt(adt_id, _substs) => {
                if self.has_drop_impl(*adt_id) {
                    return true;
                }
                match self.adt_def(*adt_id) {
                    Some(adt) => {
                        if adt.kind == AdtKind::Union {
                            return true;
                        }
                        adt.variants.iter().any(|v| {
                            v.fields
                                .iter()
                                .any(|f| self.needs_drop_rec(f.ty, visited))
                        })
                    }
                    // Unregistered ADT: cannot inspect fields. Err toward no drop
                    // (see doc comment) rather than a spurious destructor.
                    None => false,
                }
            }
            _ => false,
        };
        visited.remove(&ty);
        result
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

/// fn_sig.
    pub fn fn_sig(&self, def_id: FnDefId) -> Option<&FnSig> {
        self.fn_sigs.get(&def_id)
    }

    /// The value type of a constant definition (e.g. `i32` for
    /// `const X: i32 = ...`). Populated by typeck when checking the const.
    pub fn const_ty(&self, def_id: ConstDefId) -> Option<Ty> {
        self.const_tys.get(&def_id).copied()
    }

/// closure_sig.
    pub fn closure_sig(&self, closure_id: ClosureId) -> Option<&FnSig> {
        self.closure_sigs.get(&closure_id)
    }

    /// Recover the synthetic `AdtId` for a closure's captured environment.
    pub fn closure_adt(&self, closure_id: ClosureId) -> Option<AdtId> {
        self.closure_adt_map.get(&closure_id).copied()
    }

/// body_ty.
    pub fn body_ty(&self, def_id: LocalDefId) -> Option<Ty> {
        self.body_tys.get(&def_id).copied()
    }

/// variant_type.
    pub fn variant_type(&self, adt_id: AdtId, variant_idx: u32) -> Ty {
        self.variant_types
            .get(&adt_id)
            .and_then(|vts| vts.get(variant_idx as usize).copied())
            .unwrap_or(Ty::ERROR)
    }
}

impl TypeLookup for TyCtx {
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
    fn opaque_hidden_ty(&self, id: OpaqueTyId) -> Option<Ty> {
        self.opaque_hidden.get(&id).copied()
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
impl TyCtx {
    /// Get the trait definition by ID.
    pub fn trait_def(&self, id: glyim_core::def_id::TraitDefId) -> Option<&crate::TraitDef> {
        self.trait_defs.get(&id)
    }
}
