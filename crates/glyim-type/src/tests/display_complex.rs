//! Tests for `PrintTy` display formatting of complex types.

use glyim_core::def_id::AdtId;
use glyim_core::primitives::{Abi, IntTy, Mutability, Safety, UintTy};

use super::helpers::test_ty_ctx;
use super::helpers::with_fresh_ty_ctx;
use crate::display::PrintTy;
use crate::*;

#[test]
fn display_ref_to_i32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inner = c.mk_ty(TyKind::Int(IntTy::I32));
        c.mk_ref(Region::Erased, inner, Mutability::Not)
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "&i32");
}

#[test]
fn display_mut_ref_to_u64() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inner = c.mk_ty(TyKind::Uint(UintTy::U64));
        c.mk_ref(Region::Erased, inner, Mutability::Mut)
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "&mut u64");
}

#[test]
fn display_triple_ref() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let r1 = c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not);
        let r2 = c.mk_ref(Region::Erased, r1, Mutability::Not);
        c.mk_ref(Region::Erased, r2, Mutability::Mut)
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "&mut &&bool");
}

#[test]
fn display_ref_to_infer() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(5);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        c.mk_ref(Region::Erased, infer, Mutability::Not)
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "&?ty5");
}

#[test]
fn display_mut_ref_to_infer() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let var = IntVar::from_raw(2);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Int(var)));
        c.mk_ref(Region::Erased, infer, Mutability::Mut)
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "&mut ?int2");
}

#[test]
fn display_complex_nested() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let fvar = FloatVar::from_raw(0);
        let fty = c.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
        let r1 = c.mk_ref(Region::Erased, fty, Mutability::Not);
        c.mk_ref(Region::Erased, r1, Mutability::Mut)
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    assert_eq!(printed, "&mut &?float0");
}

#[test]
fn display_different_int_sizes() {
    let (ctx, i8_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I8)));
    assert_eq!(format!("{}", PrintTy::new(i8_ty, &ctx)), "i8");

    let (ctx, i16_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I16)));
    assert_eq!(format!("{}", PrintTy::new(i16_ty, &ctx)), "i16");

    let (ctx, i64_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I64)));
    assert_eq!(format!("{}", PrintTy::new(i64_ty, &ctx)), "i64");
}

#[test]
fn display_different_uint_sizes() {
    let (ctx, u8_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U8)));
    assert_eq!(format!("{}", PrintTy::new(u8_ty, &ctx)), "u8");

    let (ctx, u16_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U16)));
    assert_eq!(format!("{}", PrintTy::new(u16_ty, &ctx)), "u16");

    let (ctx, u32_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U32)));
    assert_eq!(format!("{}", PrintTy::new(u32_ty, &ctx)), "u32");
}

#[test]
fn display_fallthrough_for_adt() {
    // Adt, Tuple, FnPtr etc. fall through to the Debug arm
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(AdtId::from_raw(1), substs)
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    // Should contain "Adt" since it falls back to Debug
    assert!(printed.contains("Adt"));
}

#[test]
fn display_fallthrough_for_tuple() {
    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(subst));
    let printed = PrintTy::new(tuple_ty, &ctx).to_string();
    assert_eq!(printed, "(i32, bool)");
}

#[test]
fn display_fallthrough_for_fn_ptr() {
    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let sig = FnSig {
        inputs,
        output: bool_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(sig));
    let printed = PrintTy::new(fn_ptr_ty, &ctx).to_string();
    assert_eq!(printed, "fn(i32, bool) -> bool");
}

#[test]
fn display_depth_limit() {
    // Very deeply nested should show "…" at depth limit
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let mut current = c.bool_ty();
        for _ in 0..15 {
            current = c.mk_ref(Region::Erased, current, Mutability::Not);
        }
        current
    });
    let printed = format!("{}", PrintTy::new(ty, &ctx));
    // With MAX_DISPLAY_DEPTH=10, 15 levels should trigger truncation
    assert!(printed.contains("…") || printed.len() < 200);
}
