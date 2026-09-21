//! Direct tests for compute_flags function: depth overflow, propagation through kinds.
use super::helpers::test_ty_ctx;
use crate::*;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::{Abi, Mutability, Safety, UintTy};

// --- S10-T08: compute_flags sets HAS_DEPTH_OVERFLOW at depth > 64 ---

#[test]
fn compute_flags_depth_overflow_direct() {
    let ctx = test_ty_ctx();
    // Call compute_flags directly with depth > 64
    let flags = compute_flags(&TyKind::Bool, &ctx, 65);
    assert!(
        flags.contains(TypeFlags::HAS_DEPTH_OVERFLOW),
        "Expected HAS_DEPTH_OVERFLOW for depth > 64"
    );
}

// --- S10-T09: HAS_DEPTH_OVERFLOW does NOT set HAS_ERROR ---

#[test]
fn compute_flags_depth_overflow_no_error() {
    let ctx = test_ty_ctx();
    let flags = compute_flags(&TyKind::Bool, &ctx, 100);
    assert!(flags.contains(TypeFlags::HAS_DEPTH_OVERFLOW));
    assert!(!flags.contains(TypeFlags::HAS_ERROR));
}

// --- S10-T06: compute_flags detects HAS_TY_INFER ---

#[test]
fn compute_flags_detects_has_ty_infer() {
    let ctx = test_ty_ctx();
    let infer_kind = TyKind::Infer(InferVar::Ty(TyVar::from_raw(0)));
    let flags = compute_flags(&infer_kind, &ctx, 0);
    assert!(
        flags.contains(TypeFlags::HAS_TY_INFER),
        "Expected HAS_TY_INFER"
    );
}

// --- S10-T07: compute_flags detects HAS_ERROR ---

#[test]
fn compute_flags_detects_has_error() {
    let ctx = test_ty_ctx();
    let error_kind = TyKind::Error;
    let flags = compute_flags(&error_kind, &ctx, 0);
    assert!(flags.contains(TypeFlags::HAS_ERROR));
}

// --- S10-T11: compute_flags propagates through Ref, Slice, Array ---

#[test]
fn compute_flags_propagates_through_ref() {
    let mut ctx = test_ty_ctx();
    let infer_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
    let ref_kind = TyKind::Ref(Region::Erased, infer_ty, Mutability::Not);
    let flags = compute_flags(&ref_kind, &ctx, 0);
    assert!(
        flags.contains(TypeFlags::HAS_TY_INFER),
        "Ref should propagate inner HAS_TY_INFER"
    );
}

#[test]
fn compute_flags_propagates_through_slice() {
    let mut ctx = test_ty_ctx();
    let infer_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
    let slice_kind = TyKind::Slice(infer_ty);
    let flags = compute_flags(&slice_kind, &ctx, 0);
    assert!(
        flags.contains(TypeFlags::HAS_TY_INFER),
        "Slice should propagate inner HAS_TY_INFER"
    );
}

#[test]
fn compute_flags_propagates_through_array() {
    let mut ctx = test_ty_ctx();
    let infer_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
    let usize_ty = ctx.mk_ty(TyKind::Uint(UintTy::Usize));
    let arr_kind = TyKind::Array(
        infer_ty,
        Const {
            kind: ConstKind::Uint(5),
            ty: usize_ty,
        },
    );
    let flags = compute_flags(&arr_kind, &ctx, 0);
    assert!(
        flags.contains(TypeFlags::HAS_TY_INFER),
        "Array should propagate inner HAS_TY_INFER"
    );
}

// --- S10-T12: compute_flags propagates through Substitution ---

#[test]
fn compute_flags_propagates_through_tuple_substitution() {
    let mut ctx = test_ty_ctx();
    let infer_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(infer_ty)]);
    let tuple_kind = TyKind::Tuple(subst);
    let flags = compute_flags(&tuple_kind, &ctx, 0);
    assert!(
        flags.contains(TypeFlags::HAS_TY_INFER),
        "Tuple should propagate HAS_TY_INFER via substitution"
    );
}

#[test]
fn compute_flags_propagates_through_adt_substitution() {
    let mut ctx = test_ty_ctx();
    let infer_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(infer_ty)]);
    let adt_kind = TyKind::Adt(AdtId::from_raw(1), subst);
    let flags = compute_flags(&adt_kind, &ctx, 0);
    assert!(
        flags.contains(TypeFlags::HAS_TY_INFER),
        "ADT should propagate HAS_TY_INFER via substitution"
    );
}

#[test]
fn compute_flags_propagates_through_fn_ptr_substitution() {
    let mut ctx = test_ty_ctx();
    let infer_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(infer_ty)]);
    let fn_sig = FnSig {
        inputs: subst,
        output: ctx.bool_ty(),
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_kind = TyKind::FnPtr(fn_sig);
    let flags = compute_flags(&fn_ptr_kind, &ctx, 0);
    assert!(
        flags.contains(TypeFlags::HAS_TY_INFER),
        "FnPtr should propagate HAS_TY_INFER via substitution"
    );
}

// --- Regression: depth 0 does not overflow ---

#[test]
fn compute_flags_depth_zero_no_overflow() {
    let ctx = test_ty_ctx();
    let flags = compute_flags(&TyKind::Bool, &ctx, 0);
    assert!(!flags.contains(TypeFlags::HAS_DEPTH_OVERFLOW));
}

// --- Regression: depth exactly 64 still OK ---

#[test]
fn compute_flags_depth_64_no_overflow() {
    let ctx = test_ty_ctx();
    let flags = compute_flags(&TyKind::Never, &ctx, 64);
    assert!(!flags.contains(TypeFlags::HAS_DEPTH_OVERFLOW));
}

// --- Regression: depth 65 triggers overflow but no other flags ---

#[test]
fn compute_flags_depth_overflow_isolated() {
    let ctx = test_ty_ctx();
    // Use a type that normally has flags (like Error) to ensure overflow overrides
    let flags = compute_flags(&TyKind::Error, &ctx, 65);
    // Overflow flag should be set, and Error flag should NOT be set because overflow short-circuits
    assert!(flags.contains(TypeFlags::HAS_DEPTH_OVERFLOW));
    assert!(!flags.contains(TypeFlags::HAS_ERROR));
}
