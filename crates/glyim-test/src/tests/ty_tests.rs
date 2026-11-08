use crate::assertions::ty::assert_ty_eq;
use crate::*;
use glyim_core::primitives::*;
use glyim_type::{Region, Ty, TyKind};

#[test]
fn test_ty_assert_is_int() {
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| ctx.mk_ty(TyKind::Int(IntTy::I32)));
    let _ = assert_ty(&ctx, ty).is_int(IntTy::I32);
}

#[test]
fn test_ty_assert_is_bool() {
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| ctx.bool_ty());
    let _ = assert_ty(&ctx, ty).is_bool();
}

#[test]
fn test_ty_assert_is_unit() {
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| ctx.unit_ty());
    let _ = assert_ty(&ctx, ty).is_unit();
}

#[test]
fn test_ty_assert_is_never() {
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| ctx.never_ty());
    let _ = assert_ty(&ctx, ty).is_never();
}

#[test]
fn test_ty_assert_is_error() {
    let ctx = test_frozen_ty_ctx();
    let _ = assert_ty(&ctx, Ty::ERROR).is_error();
}

#[test]
fn test_ty_assert_chained_ref() {
    let mut ctx_mut = test_ty_ctx();
    let inner = ctx_mut.bool_ty();
    let ref_ty = ctx_mut.mk_ref(Region::Erased, inner, Mutability::Mut);
    let ctx = ctx_mut.freeze();
    let _ = assert_ty(&ctx, ref_ty).is_ref(Mutability::Mut).is_bool();
}

#[test]
fn test_ty_assert_chained_ref_immut() {
    let mut ctx_mut = test_ty_ctx();
    let inner = ctx_mut.unit_ty();
    let ref_ty = ctx_mut.mk_ref(Region::Erased, inner, Mutability::Not);
    let ctx = ctx_mut.freeze();
    let _ = assert_ty(&ctx, ref_ty).is_ref(Mutability::Not).is_unit();
}

#[test]
fn test_ty_assert_uint() {
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| ctx.mk_ty(TyKind::Uint(UintTy::U32)));
    let _ = assert_ty(&ctx, ty).is_uint(UintTy::U32);
}

#[test]
fn test_ty_assert_float() {
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| ctx.mk_ty(TyKind::Float(FloatTy::F64)));
    let _ = assert_ty(&ctx, ty).is_float(FloatTy::F64);
}

#[test]
fn test_sentinel_constants() {
    let ctx = test_frozen_ty_ctx();
    assert!(matches!(ctx.ty_kind(Ty::ERROR), TyKind::Error));
    assert!(matches!(ctx.ty_kind(Ty::NEVER), TyKind::Never));
    assert!(matches!(ctx.ty_kind(Ty::UNIT), TyKind::Unit));
    assert!(matches!(ctx.ty_kind(Ty::BOOL), TyKind::Bool));
}

#[test]
fn test_check_ty_composable_ok() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    let result = check_ty(&ctx, ty).is_bool().is_not_error().finish();
    assert!(result.is_ok());
}

#[test]
fn test_check_ty_composable_fail() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    let result = check_ty(&ctx, ty).is_int(IntTy::I32).is_unit().finish();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().len(), 2);
}

#[test]
fn test_assert_ty_eq_same() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    assert_ty_eq(&ctx, ty, ty);
}

#[test]
fn test_layout_assertion_bool() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    assert_layout(&ctx, ty, 1, 1);
}

#[test]
fn test_layout_assertion_i32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    assert_layout(&ctx, ty, 4, 4);
}

#[test]
fn test_layout_assertion_u8() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U8)));
    assert_layout(&ctx, ty, 1, 1);
}

#[test]
fn test_layout_assertion_f64() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Float(FloatTy::F64)));
    assert_layout(&ctx, ty, 8, 8);
}

#[test]
fn test_layout_assertion_unit() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.unit_ty());
    assert_layout(&ctx, ty, 0, 1);
}

#[test]
fn test_layout_assertion_never() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.never_ty());
    assert_layout(&ctx, ty, 0, 1);
}

#[test]
fn test_layout_ref_size() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        c.mk_ref(Region::Erased, inner, Mutability::Not)
    });
    assert_layout(&ctx, ty, 8, 8);
}

#[test]
fn test_ty_factory() {
    let mut ctx = test_ty_ctx();
    let b = fixtures::TyFactory::bool(&mut ctx);
    let i = fixtures::TyFactory::i32(&mut ctx);
    let u = fixtures::TyFactory::u32(&mut ctx);
    let f = fixtures::TyFactory::f64(&mut ctx);
    let n = fixtures::TyFactory::never(&mut ctx);
    let un = fixtures::TyFactory::unit(&mut ctx);
    let r = fixtures::TyFactory::ref_to(&mut ctx, b, Mutability::Not);
    let s = fixtures::TyFactory::slice_of(&mut ctx, i);
    let frozen = ctx.freeze();
    assert!(matches!(frozen.ty_kind(b), TyKind::Bool));
    assert!(matches!(frozen.ty_kind(i), TyKind::Int(IntTy::I32)));
    assert!(matches!(frozen.ty_kind(u), TyKind::Uint(UintTy::U32)));
    assert!(matches!(frozen.ty_kind(f), TyKind::Float(FloatTy::F64)));
    assert!(matches!(frozen.ty_kind(n), TyKind::Never));
    assert!(matches!(frozen.ty_kind(un), TyKind::Unit));
    assert!(matches!(
        frozen.ty_kind(r),
        TyKind::Ref(_, _, Mutability::Not)
    ));
    assert!(matches!(frozen.ty_kind(s), TyKind::Slice(_)));
}
