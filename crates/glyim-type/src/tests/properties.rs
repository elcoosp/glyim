//! Property-based style tests for type system invariants.

use glyim_core::def_id::AdtId;
use glyim_core::primitives::{IntTy, Mutability, UintTy};

use super::helpers::{test_frozen_ty_ctx, with_fresh_ty_ctx};
use crate::*;

#[test]
fn property_sentinels_are_always_accessible() {
    let ctx = test_frozen_ty_ctx();
    // No matter what operations preceded, sentinels must be at their indices
    assert_eq!(ctx.error_ty(), Ty::ERROR);
    assert_eq!(ctx.never_ty(), Ty::NEVER);
    assert_eq!(ctx.unit_ty(), Ty::UNIT);
    assert_eq!(ctx.bool_ty(), Ty::BOOL);
}

#[test]
fn property_ty_roundtrip_via_index() {
    let (frozen, tys) = with_fresh_ty_ctx(|c| {
        let t1 = c.mk_ty(TyKind::Int(IntTy::I32));
        let t2 = c.mk_ty(TyKind::Uint(UintTy::U64));
        let t3 = c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not);
        vec![t1, t2, t3]
    });
    for ty in &tys {
        let raw = ty.to_raw();
        let reconstructed = Ty::from_usize(raw as usize);
        assert_eq!(*ty, reconstructed);
        assert_eq!(frozen.ty_kind(*ty), frozen.ty_kind(reconstructed));
    }
}

#[test]
fn property_type_flags_consistent_with_kind() {
    let (frozen, tys) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_usize(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let ref_to_infer = c.mk_ref(Region::Erased, infer, Mutability::Not);
        let ref_to_i32 = c.mk_ref(Region::Erased, i32_ty, Mutability::Not);
        vec![
            Ty::ERROR,
            Ty::BOOL,
            Ty::NEVER,
            Ty::UNIT,
            infer,
            i32_ty,
            ref_to_infer,
            ref_to_i32,
        ]
    });
    // Ty::ERROR should have HAS_ERROR
    assert!(frozen.ty_flags(tys[0]).contains(TypeFlags::HAS_ERROR));
    // Plain types should have no flags
    assert!(frozen.ty_flags(tys[1]).is_empty());
    assert!(frozen.ty_flags(tys[2]).is_empty());
    assert!(frozen.ty_flags(tys[3]).is_empty());
    // Infer should have HAS_TY_INFER
    assert!(frozen.ty_flags(tys[4]).contains(TypeFlags::HAS_TY_INFER));
    // Plain i32 should have no flags
    assert!(frozen.ty_flags(tys[5]).is_empty());
    // Ref to infer should have HAS_TY_INFER
    assert!(frozen.ty_flags(tys[6]).contains(TypeFlags::HAS_TY_INFER));
    // Ref to i32 should have no flags
    assert!(frozen.ty_flags(tys[7]).is_empty());
}

#[test]
fn property_substitution_dedup_is_consistent() {
    let (_ctx, subs) = with_fresh_ty_ctx(|c| {
        let args1 = vec![GenericArg::Ty(c.bool_ty())];
        let args2 = vec![GenericArg::Ty(c.bool_ty())];
        let args3 = vec![GenericArg::Ty(c.mk_ty(TyKind::Int(IntTy::I32)))];
        let s1 = c.intern_substitution(args1);
        let s2 = c.intern_substitution(args2);
        let s3 = c.intern_substitution(args3);
        (s1, s2, s3)
    });
    // Same args => same index
    assert_eq!(subs.0.index(), subs.1.index());
    // Different args => different index
    assert_ne!(subs.0.index(), subs.2.index());
}

#[test]
fn property_freeze_preserves_all_data() {
    let (frozen, data) = with_fresh_ty_ctx(|c| {
        let t_bool = c.bool_ty();
        let t_i32 = c.mk_ty(TyKind::Int(IntTy::I32));
        let t_ref = c.mk_ref(Region::Erased, t_bool, Mutability::Mut);
        let substs = c.intern_substitution(vec![GenericArg::Ty(t_i32)]);
        let t_adt = c.mk_adt(AdtId::from_usize(42), substs);
        let vid = c.new_region_var(Region::Static);
        (t_bool, t_i32, t_ref, t_adt, vid)
    });
    let (t_bool, t_i32, t_ref, t_adt, vid) = data;

    // All types are accessible with correct kinds
    assert!(matches!(frozen.ty_kind(t_bool), TyKind::Bool));
    assert!(matches!(frozen.ty_kind(t_i32), TyKind::Int(IntTy::I32)));
    assert!(matches!(
        frozen.ty_kind(t_ref),
        TyKind::Ref(_, _, Mutability::Mut)
    ));
    if let TyKind::Adt(id, substs) = frozen.ty_kind(t_adt) {
        assert_eq!(id.to_raw(), 42);
        assert_eq!(substs.len(), 1);
    } else {
        panic!("expected Adt");
    }
    // Region vars are preserved
    assert!(matches!(frozen.region(vid), Region::Static));
}

#[test]
fn property_ty_equality_by_index() {
    // Ty equality is by raw index
    let t1 = Ty::from_usize(42);
    let t2 = Ty::from_usize(42);
    let t3 = Ty::from_usize(43);
    assert_eq!(t1, t2);
    assert_ne!(t1, t3);
}

#[test]
fn property_ty_ordering_by_index() {
    let t1 = Ty::from_usize(1);
    let t2 = Ty::from_usize(2);
    assert!(t1 < t2);
    assert!(t2 > t1);
}

#[test]
fn property_substitution_len_matches_args() {
    let (ctx, sub) = with_fresh_ty_ctx(|c| {
        let args = vec![
            GenericArg::Ty(c.bool_ty()),
            GenericArg::Lifetime(Region::Erased),
            GenericArg::Ty(c.mk_ty(TyKind::Int(IntTy::I32))),
        ];
        c.intern_substitution(args)
    });
    assert_eq!(sub.len() as usize, ctx.substitution_args(sub).len());
}

#[test]
fn property_empty_substitution_deduplicates() {
    let (ctx, (s1, s2, s3)) = with_fresh_ty_ctx(|c| {
        let s1 = c.intern_substitution(vec![]);
        let s2 = c.intern_substitution(vec![]);
        let s3 = c.intern_substitution(vec![]);
        (s1, s2, s3)
    });
    assert_eq!(s1.index(), s2.index());
    assert_eq!(s2.index(), s3.index());
    assert!(s1.is_empty());
    let _ = ctx;
}

#[test]
fn property_error_propagates_through_all_wrappers() {
    let (frozen, ref_ty) =
        with_fresh_ty_ctx(|c| c.mk_ref(Region::Erased, Ty::ERROR, Mutability::Not));
    let (frozen2, raw_ty) =
        with_fresh_ty_ctx(|c| c.mk_ty(TyKind::RawPtr(Ty::ERROR, Mutability::Not)));
    let (frozen3, slice_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Slice(Ty::ERROR)));
    let (frozen4, inner) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(Ty::ERROR)]);
        c.mk_adt(AdtId::from_usize(1), substs)
    });
    assert!(frozen.ty_flags(ref_ty).contains(TypeFlags::HAS_ERROR));
    assert!(frozen2.ty_flags(raw_ty).contains(TypeFlags::HAS_ERROR));
    assert!(frozen3.ty_flags(slice_ty).contains(TypeFlags::HAS_ERROR));
    assert!(frozen4.ty_flags(inner).contains(TypeFlags::HAS_ERROR));
}
