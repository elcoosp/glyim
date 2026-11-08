//! Tests for `PrintTy` display formatting.

use glyim_core::primitives::{FloatTy, IntTy, Mutability, UintTy};

use super::helpers::test_ty_ctx;
use super::helpers::{test_frozen_ty_ctx, with_fresh_ty_ctx};
use crate::display::PrintTy;
use crate::*;

#[test]
fn display_bool() {
    let ctx = test_frozen_ty_ctx();
    let printed = format!("{}", PrintTy::new(Ty::BOOL, &ctx));
    assert_eq!(printed, "bool");
}

#[test]
fn display_never() {
    let ctx = test_frozen_ty_ctx();
    let printed = format!("{}", PrintTy::new(Ty::NEVER, &ctx));
    assert_eq!(printed, "!");
}

#[test]
fn display_unit() {
    let ctx = test_frozen_ty_ctx();
    let printed = format!("{}", PrintTy::new(Ty::UNIT, &ctx));
    assert_eq!(printed, "()");
}

#[test]
fn display_error() {
    let ctx = test_frozen_ty_ctx();
    let printed = format!("{}", PrintTy::new(Ty::ERROR, &ctx));
    assert_eq!(printed, "<error>");
}

#[test]
fn display_int_i32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "i32");
}

#[test]
fn display_int_i64() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I64)));
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "i64");
}

#[test]
fn display_int_isize() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::Isize)));
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "isize");
}

#[test]
fn display_uint_u32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U32)));
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "u32");
}

#[test]
fn display_uint_usize() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::Usize)));
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "usize");
}

#[test]
fn display_float_f32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Float(FloatTy::F32)));
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "f32");
}

#[test]
fn display_float_f64() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Float(FloatTy::F64)));
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "f64");
}

#[test]
fn display_ref_mut() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Mut));
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "&mut bool");
}

#[test]
fn display_ref_shared() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not));
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "&bool");
}

#[test]
fn display_nested_ref() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inner = c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not);
        c.mk_ref(Region::Erased, inner, Mutability::Mut)
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "&mut &bool");
}

#[test]
fn display_infer_ty_var() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        c.mk_ty(TyKind::Infer(InferVar::Ty(var)))
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "?ty0");
}

#[test]
fn display_infer_int_var() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let var = IntVar::from_raw(3);
        c.mk_ty(TyKind::Infer(InferVar::Int(var)))
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "?int3");
}

#[test]
fn display_infer_float_var() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let var = FloatVar::from_raw(7);
        c.mk_ty(TyKind::Infer(InferVar::Float(var)))
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "?float7");
}

#[test]
fn display_ty_debug() {
    let ty = Ty::BOOL;
    let debug = format!("{ty:?}");
    assert_eq!(debug, "Ty(3)");
}

#[test]
fn display_unknown_kind_falls_back_to_debug() {
    let mut ctx = test_ty_ctx();
    let char_ty = ctx.mk_ty(TyKind::Char);
    let printed = PrintTy::new(char_ty, &ctx).to_string();
    assert_eq!(printed, "char");
}
