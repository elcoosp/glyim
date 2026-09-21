use crate::display::TypeLookup;
use crate::substitution::GenericArg;
use crate::ty::*;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::Mutability;
use std::collections::{HashMap, HashSet};

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    #[doc = "Auto-trait capability flags."]
    pub struct AutoTraitFlags: u8 {
        #[doc = "Type implements `Send`."]
        const SEND  = 1 << 0;
        #[doc = "Type implements `Sync`."]
        const SYNC  = 1 << 1;
        #[doc = "Type implements `Unpin`."]
        const UNPIN = 1 << 2;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// AutoTrait.
pub enum AutoTrait {
    /// Variant.
    Send,
    /// Variant.
    Sync,
    /// Variant.
    Unpin,
}

impl AutoTrait {
    /// flag.
    pub fn flag(self) -> AutoTraitFlags {
        match self {
            AutoTrait::Send => AutoTraitFlags::SEND,
            AutoTrait::Sync => AutoTraitFlags::SYNC,
            AutoTrait::Unpin => AutoTraitFlags::UNPIN,
        }
    }

    /// ALL.
    pub const ALL: [AutoTrait; 3] = [AutoTrait::Send, AutoTrait::Sync, AutoTrait::Unpin];
}

#[derive(Clone, Debug, Default)]
/// AdtRepr.
pub struct AdtRepr {
    /// Struct.
    pub field_tys: Vec<Ty>,
}

impl AdtRepr {
    /// new.
    pub fn new(field_tys: Vec<Ty>) -> Self {
        Self { field_tys }
    }
}

#[derive(Clone, Debug, Default)]
/// AutoTraitRegistry.
pub struct AutoTraitRegistry {
    negative_impls: HashSet<(AdtId, AutoTrait)>,
    manual_impls: HashSet<(AdtId, AutoTrait)>,
}

impl AutoTraitRegistry {
    /// new.
    pub fn new() -> Self {
        Self::default()
    }

    /// register_negative_impl.
    pub fn register_negative_impl(&mut self, adt_id: AdtId, auto_trait: AutoTrait) {
        self.negative_impls.insert((adt_id, auto_trait));
    }

    /// register_manual_impl.
    pub fn register_manual_impl(&mut self, adt_id: AdtId, auto_trait: AutoTrait) {
        self.manual_impls.insert((adt_id, auto_trait));
    }

    /// has_negative_impl.
    pub fn has_negative_impl(&self, adt_id: AdtId, auto_trait: AutoTrait) -> bool {
        self.negative_impls.contains(&(adt_id, auto_trait))
    }

    /// has_manual_impl.
    pub fn has_manual_impl(&self, adt_id: AdtId, auto_trait: AutoTrait) -> bool {
        self.manual_impls.contains(&(adt_id, auto_trait))
    }
}

/// Compute auto trait flags for a type.
///
/// Uses coinductive semantics: recursive types are assumed to implement
/// all auto traits until proven otherwise.
pub fn compute_auto_traits(
    ty: Ty,
    lookup: &dyn TypeLookup,
    registry: &AutoTraitRegistry,
    adt_reprs: &HashMap<AdtId, AdtRepr>,
) -> AutoTraitFlags {
    let mut cache = HashMap::new();
    let mut evaluating = HashSet::new();
    compute_auto_traits_recursive(ty, lookup, registry, adt_reprs, &mut cache, &mut evaluating)
}

fn compute_auto_traits_recursive(
    ty: Ty,
    lookup: &dyn TypeLookup,
    registry: &AutoTraitRegistry,
    adt_reprs: &HashMap<AdtId, AdtRepr>,
    cache: &mut HashMap<Ty, AutoTraitFlags>,
    evaluating: &mut HashSet<Ty>,
) -> AutoTraitFlags {
    if let Some(&flags) = cache.get(&ty) {
        return flags;
    }

    if evaluating.contains(&ty) {
        return AutoTraitFlags::all();
    }

    evaluating.insert(ty);

    let flags = compute_auto_traits_for_kind(ty, lookup, registry, adt_reprs, cache, evaluating);

    evaluating.remove(&ty);
    cache.insert(ty, flags);
    flags
}

fn compute_auto_traits_for_kind(
    ty: Ty,
    lookup: &dyn TypeLookup,
    registry: &AutoTraitRegistry,
    adt_reprs: &HashMap<AdtId, AdtRepr>,
    cache: &mut HashMap<Ty, AutoTraitFlags>,
    evaluating: &mut HashSet<Ty>,
) -> AutoTraitFlags {
    match lookup.ty_kind(ty) {
        TyKind::Bool
        | TyKind::Int(_)
        | TyKind::Uint(_)
        | TyKind::Float(_)
        | TyKind::Char
        | TyKind::Never
        | TyKind::Unit
        | TyKind::String => AutoTraitFlags::all(),

        TyKind::Ref(_, inner, Mutability::Not) => {
            let inner_flags = compute_auto_traits_recursive(
                *inner, lookup, registry, adt_reprs, cache, evaluating,
            );
            let mut flags = AutoTraitFlags::UNPIN;
            if inner_flags.contains(AutoTraitFlags::SYNC) {
                flags |= AutoTraitFlags::SEND | AutoTraitFlags::SYNC;
            }
            flags
        }

        TyKind::Ref(_, inner, Mutability::Mut) => {
            let inner_flags = compute_auto_traits_recursive(
                *inner, lookup, registry, adt_reprs, cache, evaluating,
            );
            let mut flags = AutoTraitFlags::UNPIN;
            if inner_flags.contains(AutoTraitFlags::SEND) {
                flags |= AutoTraitFlags::SEND;
            }
            if inner_flags.contains(AutoTraitFlags::SYNC) {
                flags |= AutoTraitFlags::SYNC;
            }
            flags
        }

        TyKind::RawPtr(_, _) => AutoTraitFlags::UNPIN,

        TyKind::Slice(inner) => {
            compute_auto_traits_recursive(*inner, lookup, registry, adt_reprs, cache, evaluating)
        }

        TyKind::Array(inner, _) => {
            compute_auto_traits_recursive(*inner, lookup, registry, adt_reprs, cache, evaluating)
        }

        TyKind::Tuple(substs) => {
            let mut flags = AutoTraitFlags::all();
            for arg in lookup.substitution_args(*substs) {
                if let GenericArg::Ty(t) = arg {
                    flags &= compute_auto_traits_recursive(
                        *t, lookup, registry, adt_reprs, cache, evaluating,
                    );
                }
            }
            flags
        }

        TyKind::Adt(adt_id, _substs) => {
            let mut flags = AutoTraitFlags::all();

            for auto_trait in AutoTrait::ALL {
                let trait_flag = auto_trait.flag();

                if registry.has_manual_impl(*adt_id, auto_trait) {
                    continue;
                }

                if registry.has_negative_impl(*adt_id, auto_trait) {
                    flags -= trait_flag;
                    continue;
                }

                // First, try AdtDef via TypeLookup (full definition with named fields).
                if let Some(def) = lookup.adt_def(*adt_id) {
                    for field in def.fields.iter() {
                        let field_flags = compute_auto_traits_recursive(
                            field.ty, lookup, registry, adt_reprs, cache, evaluating,
                        );
                        if !field_flags.contains(trait_flag) {
                            flags -= trait_flag;
                            break;
                        }
                    }
                } else if let Some(repr) = adt_reprs.get(adt_id) {
                    // Fall back to AdtRepr (field type list only).
                    for &field_ty in &repr.field_tys {
                        let field_flags = compute_auto_traits_recursive(
                            field_ty, lookup, registry, adt_reprs, cache, evaluating,
                        );
                        if !field_flags.contains(trait_flag) {
                            flags -= trait_flag;
                            break;
                        }
                    }
                } else {
                    // No definition or repr registered — cannot determine auto traits.
                    flags = AutoTraitFlags::empty();
                    break;
                }
            }

            flags
        }

        TyKind::FnPtr(_) | TyKind::FnDef(_, _) => AutoTraitFlags::all(),

        TyKind::Closure(_, substs) => {
            let mut flags = AutoTraitFlags::all();
            for arg in lookup.substitution_args(*substs) {
                if let GenericArg::Ty(t) = arg {
                    let inner = compute_auto_traits_recursive(
                        *t, lookup, registry, adt_reprs, cache, evaluating,
                    );
                    flags &= inner;
                }
            }
            flags
        }

        TyKind::Dynamic(_, _) => AutoTraitFlags::empty(),

        TyKind::Opaque(_, _) | TyKind::Projection(_) => {
            // Plan §7.2: an opaque type (`impl Trait` return position) must
            // delegate its auto-trait set to its *defining-use* concrete type
            // rather than stopping at the wrapper with zero auto traits. The
            // hidden type is provided by the `TypeLookup` (populated by typeck
            // at the defining site); if known, recurse into it. The `Projection`
            // half (generic/normalized associated-type projections) requires
            // trait-solver normalization (§8/§9.4) and is intentionally left as
            // empty here.
            if let TyKind::Opaque(id, _) = lookup.ty_kind(ty)
                && let Some(hidden) = lookup.opaque_hidden_ty(*id)
            {
                return compute_auto_traits_recursive(
                    hidden, lookup, registry, adt_reprs, cache, evaluating,
                );
            }
            AutoTraitFlags::empty()
        }

        TyKind::Infer(_) | TyKind::Param(_) | TyKind::Bound(_, _) | TyKind::Error => {
            AutoTraitFlags::empty()
        }
    }
}
