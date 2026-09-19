//! Cast legality — the single source of truth shared by type-checking and
//! constant evaluation (de-stubbing plan §13.2).
//!
//! `is_valid_cast` decides whether a value of type `from` may be cast to type
//! `to`. It is a pure function of the two types (and the `TypeLookup` needed to
//! inspect ADT definitions); it performs no value transformation. Both
//! `glyim-typeck` (which emits an error for illegal casts) and
//! `glyim-const-eval` (which must reject illegal `const` casts) delegate to it
//! so the rules live in exactly one place.

use crate::adt_def::AdtKind;
use crate::display::TypeLookup;
use crate::ty::InferVar;
use crate::{Ty, TyKind};

// The following are referenced only by the `#[cfg(test)]` module below
// (via `use super::*`). Gated so the non-test lib build stays warning-free.
#[cfg(test)]
use crate::adt_def::{AdtDef, FieldDef, VariantDef};
#[cfg(test)]
use glyim_core::arena::IndexVec;
#[cfg(test)]
use glyim_core::def_id::AdtId;

/// Return `true` if a value of type `from` may be cast to type `to`.
///
/// The rules mirror Rust's `as` cast legality (subset implemented so far):
///   * int/uint ↔ int/uint/float
///   * float ↔ float/int/uint
///   * raw pointer / reference ↔ raw pointer / int
///   * bool ↔ int/uint
///   * char ↔ int/uint
///   * a fieldless (C-like) enum ↔ int/uint
///   * identical types are always allowed
pub fn is_valid_cast(ctx: &dyn TypeLookup, from: Ty, to: Ty) -> bool {
    use TyKind::*;
    let from_k = ctx.ty_kind(from);
    let to_k = ctx.ty_kind(to);
    match (from_k, to_k) {
        // A numeric *inference variable* (`{integer}` / `{float}`) unifies
        // with any concrete numeric target, so `(x as u16)` where `x: {integer}`
        // is valid. Without these arms, every `as u16` / `as u8` on a
        // literal-inferred integer in the stdlib reported `invalid cast`.
        (Infer(InferVar::Int(_)), Int(_) | Uint(_) | Float(_)) => true,
        (Infer(InferVar::Float(_)), Int(_) | Uint(_) | Float(_)) => true,
        (Int(_) | Uint(_), Int(_) | Uint(_) | Float(_)) => true,
        (Float(_), Float(_) | Int(_) | Uint(_)) => true,
        (RawPtr(_, _) | Ref(_, _, _), RawPtr(_, _) | Int(_)) => true,
        (Bool, Int(_) | Uint(_)) => true,
        (Char, Int(_) | Uint(_)) => true,
        (Adt(from_id, _), Int(_) | Uint(_)) => {
            // Two legal shapes:
            //   1. A fieldless (C-like) enum → any integer (plan §13.2).
            //   2. A single-field newtype struct whose sole field is itself
            //      a scalar (int/uint/char/bool) → any integer. This is what
            //      the stdlib relies on for `ThreadId(id: u64) as usize`;
            //      without it, every such cast reports "invalid cast".
            // Structs with more than one field, and enums with data, are
            // rejected.
            match ctx.adt_def(*from_id) {
                Some(adt) => {
                    if adt.kind == AdtKind::Enum
                        && adt.variants.iter().all(|v| v.fields.is_empty())
                    {
                        return true;
                    }
                    if adt.kind == AdtKind::Struct && adt.fields.len() == 1 {
                        if let Some(f) = adt.fields.iter().next() {
                            if matches!(
                                ctx.ty_kind(f.ty),
                                Int(_) | Uint(_) | Char | Bool
                            ) {
                                return true;
                            }
                        }
                    }
                    false
                }
                None => false,
            }
        }
        // Type-param transmute: the stdlib writes `unsafe { x as T }` where
        // `x` is a concrete ADT and `T` is a generic parameter of the
        // enclosing impl/fn (e.g. `Mutex<T>::into_inner` unwraps its
        // `UnsafeCell<MutexInner>` and reinterprets the inner as `T`).
        // glyim's `as` therefore behaves like a transmute when the target is
        // a type parameter. Allowing it here keeps the stdlib's
        // `mem::transmute`-shaped idiom well-typed; genuinely-illegal casts
        // (e.g. `Adt → float`) remain rejected by the arms above.
        (_, Param(_)) => true,
        (Param(_), _) => true,
        _ if from == to => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty_ctx_mut::TyCtxMut;
    use glyim_core::interner::Interner;
    use glyim_core::primitives::{FloatTy, IntTy, UintTy};

    #[test]
    fn int_to_int_is_valid() {
        let mut tcx_mut = TyCtxMut::new(Interner::new());
        let i32 = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let u8 = tcx_mut.mk_ty(TyKind::Uint(UintTy::U8));
        assert!(is_valid_cast(&tcx_mut, i32, u8));
    }

    #[test]
    fn float_to_int_is_valid() {
        let mut tcx_mut = TyCtxMut::new(Interner::new());
        let f64 = tcx_mut.mk_ty(TyKind::Float(FloatTy::F64));
        let i32 = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
        assert!(is_valid_cast(&tcx_mut, f64, i32));
    }

    #[test]
    fn ptr_to_float_is_invalid() {
        let mut tcx_mut = TyCtxMut::new(Interner::new());
        let i32 = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let ptr = tcx_mut.mk_ty(TyKind::RawPtr(i32, glyim_core::primitives::Mutability::Not));
        let f64 = tcx_mut.mk_ty(TyKind::Float(FloatTy::F64));
        assert!(!is_valid_cast(&tcx_mut, ptr, f64));
    }

    #[test]
    fn identical_types_are_valid() {
        let mut tcx_mut = TyCtxMut::new(Interner::new());
        let i32 = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
        assert!(is_valid_cast(&tcx_mut, i32, i32));
    }

    #[test]
    fn fieldless_enum_to_int_is_valid() {
        // Plan §13.2: a fieldless (C-like) enum must be castable to an integer.
        // The previous version of this test was `#[ignore]`d because it passed
        // `error_ty()` (no Adt arm to exercise); here we register a real
        // fieldless enum ADT so `is_valid_cast`'s Adt arm actually runs.
        let mut tcx_mut = TyCtxMut::new(Interner::new());
        let enum_id = AdtId::from_raw(501);
        let variants = vec![VariantDef {
            name: tcx_mut.resolver().intern("A"),
    style: crate::adt_def::VariantStyle::Unit,
            fields: IndexVec::new(),
        }];
        let enum_def = AdtDef {
            kind: AdtKind::Enum,
            fields: IndexVec::new(),
            variants,
            generic_params: vec![],
};
        tcx_mut.register_adt(enum_id, enum_def);
        let substs = tcx_mut.intern_substitution(vec![]);
        let enum_ty = tcx_mut.mk_adt(enum_id, substs);
        let u8 = tcx_mut.mk_ty(TyKind::Uint(UintTy::U8));
        assert!(
            is_valid_cast(&tcx_mut, enum_ty, u8),
            "fieldless enum must be castable to u8"
        );
    }

    #[test]
    fn enum_with_data_to_int_is_invalid() {
        // Plan §13.2: an enum carrying data (a variant with a field) is NOT a
        // fieldless enum, so its cast to an integer must be rejected.
        let mut tcx_mut = TyCtxMut::new(Interner::new());
        let enum_id = AdtId::from_raw(502);
        let u8 = tcx_mut.mk_ty(TyKind::Uint(UintTy::U8));
        let field = FieldDef {
            name: tcx_mut.resolver().intern("x"),
            ty: u8,
        };
        let mut field_list = IndexVec::new();
        field_list.push(field);
        let variants = vec![VariantDef {
            name: tcx_mut.resolver().intern("A"),
    style: crate::adt_def::VariantStyle::Unit,
            fields: field_list,
        }];
        let enum_def = AdtDef {
            kind: AdtKind::Enum,
            fields: IndexVec::new(),
            variants,
            generic_params: vec![],
};
        tcx_mut.register_adt(enum_id, enum_def);
        let substs = tcx_mut.intern_substitution(vec![]);
        let enum_ty = tcx_mut.mk_adt(enum_id, substs);
        assert!(
            !is_valid_cast(&tcx_mut, enum_ty, u8),
            "enum with data must NOT be castable to u8"
        );
    }
}
