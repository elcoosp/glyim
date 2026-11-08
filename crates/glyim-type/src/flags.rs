use crate::display::TypeLookup;
use crate::predicate::Predicate;
use crate::region::Region;
use crate::substitution::GenericArg;
use crate::ty::TyKind;
use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    #[doc = "Flags summarizing properties of a type."]
    pub struct TypeFlags: u32 {
        #[doc = "Type contains an inference variable for a type."]
        const HAS_TY_INFER       = 1 << 0;
        #[doc = "Type contains a type parameter."]
        const HAS_TY_PARAM       = 1 << 1;
        #[doc = "Type contains an inference variable for a region."]
        const HAS_RE_INFER       = 1 << 2;
        #[doc = "Type contains a region parameter."]
        const HAS_RE_PARAM       = 1 << 3;
        #[doc = "Type contains an inference variable for a const."]
        const HAS_CT_INFER       = 1 << 4;
        #[doc = "Type contains a const parameter."]
        const HAS_CT_PARAM       = 1 << 5;
        #[doc = "Type contains an error."]
        const HAS_ERROR          = 1 << 7;
        #[doc = "Type contains a depth overflow."]
        const HAS_DEPTH_OVERFLOW = 1 << 8;
        #[doc = "Type contains a region placeholder."]
        const HAS_RE_PLACEHOLDER = 1 << 9;
        #[doc = "Type contains a type placeholder."]
        const HAS_TY_PLACEHOLDER = 1 << 10;
        #[doc = "Type contains interior mutability."]
        const HAS_INTERIOR_MUTABILITY = 1 << 11;
    }
}

/// `compute_flags`.
pub fn compute_flags(kind: &TyKind, ctx: &dyn TypeLookup, depth: u32) -> TypeFlags {
    const MAX_DEPTH: u32 = 64;
    if depth > MAX_DEPTH {
        tracing::warn!(
            "compute_flags exceeded depth limit at depth {}; TyKind summary: {:?}",
            depth,
            kind
        );
        return TypeFlags::HAS_DEPTH_OVERFLOW;
    }
    let mut flags = TypeFlags::empty();
    match kind {
        TyKind::Infer(_) => flags |= TypeFlags::HAS_TY_INFER,
        TyKind::Param(_) => flags |= TypeFlags::HAS_TY_PARAM,
        TyKind::Error => flags |= TypeFlags::HAS_ERROR,
        TyKind::Bound(_, _) => flags |= TypeFlags::HAS_TY_PLACEHOLDER,
        TyKind::Ref(region, ty, _) => {
            flags |= ctx.ty_flags(*ty);
            match region {
                Region::Var(_) => flags |= TypeFlags::HAS_RE_INFER,
                Region::EarlyBound(_) => flags |= TypeFlags::HAS_RE_PARAM,
                Region::Placeholder(_) => flags |= TypeFlags::HAS_RE_PLACEHOLDER,
                _ => {}
            }
        }
        TyKind::RawPtr(ty, _) => flags |= ctx.ty_flags(*ty),
        TyKind::Slice(ty) => flags |= ctx.ty_flags(*ty),
        TyKind::Array(ty, _) => flags |= ctx.ty_flags(*ty),
        TyKind::Adt(adt_id, substs) => {
            for arg in ctx.substitution_args(*substs) {
                if let GenericArg::Ty(t) = arg {
                    flags |= ctx.ty_flags(*t);
                }
            }
            if ctx.is_interior_mutable_adt(*adt_id) {
                flags |= TypeFlags::HAS_INTERIOR_MUTABILITY;
            }
        }
        TyKind::FnDef(_, substs)
        | TyKind::Closure(_, substs)
        | TyKind::Tuple(substs)
        | TyKind::Opaque(_, substs) => {
            for arg in ctx.substitution_args(*substs) {
                if let GenericArg::Ty(t) = arg {
                    flags |= ctx.ty_flags(*t);
                }
            }
        }
        TyKind::Projection(proj) => {
            for arg in ctx.substitution_args(proj.trait_ref.substs) {
                if let GenericArg::Ty(t) = arg {
                    flags |= ctx.ty_flags(*t);
                }
            }
        }
        TyKind::FnPtr(sig) => {
            for arg in ctx.substitution_args(sig.inputs) {
                if let GenericArg::Ty(t) = arg {
                    flags |= ctx.ty_flags(*t);
                }
            }
            flags |= ctx.ty_flags(sig.output);
        }
        TyKind::Dynamic(preds, region) => {
            // Walk predicates for flags
            for pred in preds.as_ref().skip_binder().as_ref() {
                if let Predicate::Trait(tp) = pred {
                    for arg in ctx.substitution_args(tp.trait_ref.substs) {
                        if let GenericArg::Ty(t) = arg {
                            flags |= ctx.ty_flags(*t);
                        }
                    }
                }
            }
            if let Region::Placeholder(_) = region {
                flags |= TypeFlags::HAS_RE_PLACEHOLDER;
            }
        }
        _ => {}
    }
    flags
}
