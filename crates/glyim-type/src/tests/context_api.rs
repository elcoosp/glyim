//! Tests for TyCtxMut and TyCtx API surface.

use glyim_core::primitives::{IntTy, Mutability};

use super::helpers::{test_frozen_ty_ctx, test_ty_ctx, with_fresh_ty_ctx};
use crate::*;

#[test]
fn new_creates_context_with_four_sentinels() {
    let ctx = test_ty_ctx();
    assert!(matches!(ctx.ty_kind(Ty::ERROR), TyKind::Error));
    assert!(matches!(ctx.ty_kind(Ty::NEVER), TyKind::Never));
    assert!(matches!(ctx.ty_kind(Ty::UNIT), TyKind::Unit));
    assert!(matches!(ctx.ty_kind(Ty::BOOL), TyKind::Bool));
}

#[test]
fn first_custom_type_is_resolvable() {
    let (frozen, custom) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    // Under the canonical interner, an `i32` maps to the shared `Ty::I32`
    // sentinel handle rather than a fresh index — what matters is that the
    // handle resolves back to the same structural type.
    assert!(matches!(frozen.ty_kind(custom), TyKind::Int(IntTy::I32)));
}

#[test]
fn multiple_distinct_adts_get_increasing_indices() {
    // Use distinct, non-primitive ADT types so each allocates a fresh,
    // increasing handle in the arena (primitives are interns/sentinels and
    // would share handles).
    let (frozen, tys) = with_fresh_ty_ctx(|c| {
        let mut mk = |id: u32| {
            let substs = c.intern_substitution(vec![]);
            c.mk_adt(glyim_core::def_id::AdtId::from_raw(1_000_000 + id), substs)
        };
        vec![mk(0), mk(1), mk(2)]
    });
    assert_ne!(tys[0], tys[1]);
    assert_ne!(tys[1], tys[2]);
    assert!(tys[0].to_raw() < tys[1].to_raw());
    assert!(tys[1].to_raw() < tys[2].to_raw());
    for ty in &tys {
        assert!(matches!(frozen.ty_kind(*ty), TyKind::Adt(_, _)));
    }
}

#[test]
fn resolver_returns_interner() {
    let ctx = test_ty_ctx();
    let name = ctx.resolver().intern("hello");
    assert_eq!(ctx.name_str(name), "hello");
}

#[test]
fn frozen_resolver_returns_interner() {
    let ctx = test_frozen_ty_ctx();
    let name = ctx.resolver().intern("world");
    assert_eq!(ctx.name_str(name), "world");
}

#[test]
fn error_ty_is_error_on_frozen_ctx() {
    let ctx = test_frozen_ty_ctx();
    assert!(ctx.ty_is_error(Ty::ERROR));
}

#[test]
fn non_error_ty_is_not_error_on_frozen_ctx() {
    let ctx = test_frozen_ty_ctx();
    assert!(!ctx.ty_is_error(Ty::BOOL));
    assert!(!ctx.ty_is_error(Ty::NEVER));
    assert!(!ctx.ty_is_error(Ty::UNIT));
}

#[test]
fn ref_to_error_is_error_on_frozen_ctx() {
    let (frozen, ref_ty) =
        with_fresh_ty_ctx(|c| c.mk_ref(Region::Erased, Ty::ERROR, Mutability::Not));
    assert!(frozen.ty_is_error(ref_ty));
}

#[test]
fn ty_has_depth_overflow_default_false() {
    let ctx = test_frozen_ty_ctx();
    assert!(!ctx.ty_has_depth_overflow(Ty::BOOL));
}

#[test]
fn type_lookup_trait_on_ty_ctx_mut() {
    let ctx = test_ty_ctx();
    let lookup: &dyn TypeLookup = &ctx;
    assert!(matches!(lookup.ty_kind(Ty::ERROR), TyKind::Error));
    assert!(!lookup.ty_flags(Ty::BOOL).contains(TypeFlags::HAS_ERROR));
    assert_eq!(lookup.error_ty(), Ty::ERROR);
}

#[test]
fn type_lookup_trait_on_ty_ctx() {
    let ctx = test_frozen_ty_ctx();
    let lookup: &dyn TypeLookup = &ctx;
    assert!(matches!(lookup.ty_kind(Ty::ERROR), TyKind::Error));
    assert!(!lookup.ty_flags(Ty::BOOL).contains(TypeFlags::HAS_ERROR));
    assert_eq!(lookup.error_ty(), Ty::ERROR);
}

#[test]
fn alloc_ty_and_mk_ty_intern_to_same_handle() {
    let (frozen, (t1, t2)) = with_fresh_ty_ctx(|c| {
        let a = c.alloc_ty(TyKind::Int(IntTy::I32));
        let b = c.mk_ty(TyKind::Int(IntTy::I32));
        (a, b)
    });
    // Canonical interner: identical structural types share one handle.
    assert_eq!(t1, t2);
    assert!(matches!(frozen.ty_kind(t1), TyKind::Int(IntTy::I32)));
    assert!(matches!(frozen.ty_kind(t2), TyKind::Int(IntTy::I32)));
}

#[test]
fn freeze_after_many_operations() {
    let (frozen, data) = with_fresh_ty_ctx(|c| {
        let t_bool = c.bool_ty();
        let t_i32 = c.mk_ty(TyKind::Int(IntTy::I32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(t_bool), GenericArg::Ty(t_i32)]);
        let adt = c.mk_adt(glyim_core::def_id::AdtId::from_raw(99), substs);
        let ref_ty = c.mk_ref(Region::Erased, adt, Mutability::Mut);
        let _vid = c.new_region_var(Region::Static);
        (t_bool, t_i32, adt, ref_ty)
    });
    let (t_bool, t_i32, adt, ref_ty) = data;
    assert!(matches!(frozen.ty_kind(t_bool), TyKind::Bool));
    assert!(matches!(frozen.ty_kind(t_i32), TyKind::Int(IntTy::I32)));
    if let TyKind::Adt(id, substs) = frozen.ty_kind(adt) {
        assert_eq!(id.to_raw(), 99);
        assert_eq!(substs.len(), 2);
    } else {
        panic!("expected Adt");
    }
    if let TyKind::Ref(region, inner, mutability) = frozen.ty_kind(ref_ty) {
        assert!(matches!(region, Region::Erased));
        assert_eq!(*inner, adt);
        assert_eq!(*mutability, Mutability::Mut);
    } else {
        panic!("expected Ref");
    }
    assert!(matches!(
        frozen.region(RegionVid::from_raw(0)),
        Region::Static
    ));
}
