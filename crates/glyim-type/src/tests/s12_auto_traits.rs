//! S12-T03: Auto trait computation identifies Send/Sync correctly.
//! Comprehensive tests for auto trait flags through `TypeLookup`, `TyCtxMut`, and `TyCtx`.

use glyim_core::def_id::AdtId;
use glyim_core::primitives::{IntTy, Mutability};

use super::helpers::with_fresh_ty_ctx;
use crate::auto_trait::{AutoTrait, AutoTraitFlags};
use crate::region::Region;
use crate::substitution::GenericArg;
use crate::ty::TyKind;

// ---- Send for primitives ----

#[test]
fn bool_is_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    assert!(ctx.implements_auto_trait(ty, AutoTrait::Send));
}

#[test]
fn i32_is_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    assert!(ctx.implements_auto_trait(ty, AutoTrait::Send));
}

#[test]
fn never_is_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.never_ty());
    assert!(ctx.implements_auto_trait(ty, AutoTrait::Send));
}

// ---- Sync for primitives ----

#[test]
fn bool_is_sync() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    assert!(ctx.implements_auto_trait(ty, AutoTrait::Sync));
}

#[test]
fn i32_is_sync() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    assert!(ctx.implements_auto_trait(ty, AutoTrait::Sync));
}

// ---- Ref types ----

#[test]
fn ref_to_send_is_send_and_sync() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.mk_ty(TyKind::Int(IntTy::I32));
        c.mk_ref(Region::Erased, inner, Mutability::Not)
    });
    assert!(ctx.implements_auto_trait(ref_ty, AutoTrait::Send));
    assert!(ctx.implements_auto_trait(ref_ty, AutoTrait::Sync));
}

#[test]
fn ref_to_non_sync_is_not_sync() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        let raw_ptr = c.mk_ty(TyKind::RawPtr(inner, Mutability::Not));
        c.mk_ref(Region::Erased, raw_ptr, Mutability::Not)
    });
    assert!(!ctx.implements_auto_trait(ref_ty, AutoTrait::Sync));
}

#[test]
fn mut_ref_to_send_is_send() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.mk_ty(TyKind::Int(IntTy::I32));
        c.mk_ref(Region::Erased, inner, Mutability::Mut)
    });
    assert!(ctx.implements_auto_trait(ref_ty, AutoTrait::Send));
    assert!(ctx.implements_auto_trait(ref_ty, AutoTrait::Sync));
}

#[test]
fn mut_ref_to_non_send_is_not_send() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        let raw_ptr = c.mk_ty(TyKind::RawPtr(inner, Mutability::Not));
        c.mk_ref(Region::Erased, raw_ptr, Mutability::Mut)
    });
    assert!(!ctx.implements_auto_trait(ref_ty, AutoTrait::Send));
}

// ---- Unpin ----

#[test]
fn primitives_are_unpin() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    assert!(ctx.implements_auto_trait(ty, AutoTrait::Unpin));
}

#[test]
fn refs_are_unpin() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        c.mk_ref(Region::Erased, inner, Mutability::Not)
    });
    assert!(ctx.implements_auto_trait(ref_ty, AutoTrait::Unpin));
}

#[test]
fn raw_ptr_is_unpin() {
    let (ctx, ptr_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        c.mk_ty(TyKind::RawPtr(inner, Mutability::Not))
    });
    assert!(ctx.implements_auto_trait(ptr_ty, AutoTrait::Unpin));
    assert!(!ctx.implements_auto_trait(ptr_ty, AutoTrait::Send));
    assert!(!ctx.implements_auto_trait(ptr_ty, AutoTrait::Sync));
}

// ---- ADT auto traits ----

#[test]
fn adt_with_send_fields_is_send() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|c| {
        let adt_id = AdtId::from_raw(300);
        let field = c.mk_ty(TyKind::Int(IntTy::I32));
        c.register_adt_repr(adt_id, vec![field]);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.implements_auto_trait(adt_ty, AutoTrait::Send));
    assert!(ctx.implements_auto_trait(adt_ty, AutoTrait::Sync));
}

#[test]
fn adt_with_non_send_field_is_not_send() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|c| {
        let adt_id = AdtId::from_raw(301);
        let inner = c.bool_ty();
        let raw_ptr = c.mk_ty(TyKind::RawPtr(inner, Mutability::Not));
        c.register_adt_repr(adt_id, vec![raw_ptr]);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(adt_id, substs)
    });
    assert!(!ctx.implements_auto_trait(adt_ty, AutoTrait::Send));
    assert!(!ctx.implements_auto_trait(adt_ty, AutoTrait::Sync));
}

#[test]
fn adt_negative_impl_removes_send() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|c| {
        let adt_id = AdtId::from_raw(302);
        let field = c.mk_ty(TyKind::Int(IntTy::I32));
        c.register_adt_repr(adt_id, vec![field]);
        c.register_negative_impl(adt_id, AutoTrait::Send);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(adt_id, substs)
    });
    assert!(!ctx.implements_auto_trait(adt_ty, AutoTrait::Send));
    assert!(
        ctx.implements_auto_trait(adt_ty, AutoTrait::Sync),
        "Sync should still be present"
    );
}

#[test]
fn adt_manual_impl_preserves_trait() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|c| {
        let adt_id = AdtId::from_raw(303);
        let inner = c.bool_ty();
        let raw_ptr = c.mk_ty(TyKind::RawPtr(inner, Mutability::Not));
        c.register_adt_repr(adt_id, vec![raw_ptr]);
        c.register_manual_impl(adt_id, AutoTrait::Send);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(adt_id, substs)
    });
    assert!(
        ctx.implements_auto_trait(adt_ty, AutoTrait::Send),
        "manual impl should override field analysis"
    );
}

#[test]
fn adt_with_no_repr_gets_no_auto_traits() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|c| {
        let adt_id = AdtId::from_raw(304);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(adt_ty);
    assert_eq!(
        flags,
        AutoTraitFlags::empty(),
        "ADT with no AdtRepr should have no auto traits"
    );
}

// ---- Tuple auto traits ----

#[test]
fn tuple_of_send_is_send() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let bool_ty = c.bool_ty();
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
        c.mk_tuple(substs)
    });
    assert!(ctx.implements_auto_trait(tuple_ty, AutoTrait::Send));
    assert!(ctx.implements_auto_trait(tuple_ty, AutoTrait::Sync));
}

#[test]
fn tuple_with_non_send_is_not_send() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let inner = c.bool_ty();
        let raw_ptr = c.mk_ty(TyKind::RawPtr(inner, Mutability::Not));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(raw_ptr)]);
        c.mk_tuple(substs)
    });
    assert!(!ctx.implements_auto_trait(tuple_ty, AutoTrait::Send));
}

// ---- Slice auto traits ----

#[test]
fn slice_of_send_is_send() {
    let (ctx, slice_ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        c.mk_ty(TyKind::Slice(i32_ty))
    });
    assert!(ctx.implements_auto_trait(slice_ty, AutoTrait::Send));
    assert!(ctx.implements_auto_trait(slice_ty, AutoTrait::Sync));
}

// ---- auto_trait_flags ----

#[test]
fn all_auto_traits_for_i32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    let flags = ctx.auto_trait_flags(ty);
    assert_eq!(flags, AutoTraitFlags::all());
}

#[test]
fn dynamic_type_has_no_auto_traits() {
    let (ctx, dyn_ty) = with_fresh_ty_ctx(|c| {
        let preds: crate::binder::Binder<Box<[crate::predicate::Predicate]>> =
            crate::binder::Binder::bind(Box::new([]), Box::new([]));
        c.mk_ty(TyKind::Dynamic(preds, Region::Erased))
    });
    let flags = ctx.auto_trait_flags(dyn_ty);
    assert_eq!(flags, AutoTraitFlags::empty());
}
