//! Tests for `TypeFlags` computation.

use glyim_core::primitives::{IntTy, Mutability, UintTy};

use super::helpers::{test_frozen_ty_ctx, with_fresh_ty_ctx};
use crate::*;

// --- HAS_TY_INFER ---

#[test]
fn infer_ty_var_sets_has_ty_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        c.mk_ty(TyKind::Infer(InferVar::Ty(var)))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn infer_int_var_sets_has_ty_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = IntVar::from_raw(0);
        c.mk_ty(TyKind::Infer(InferVar::Int(var)))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn infer_float_var_sets_has_ty_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = FloatVar::from_raw(0);
        c.mk_ty(TyKind::Infer(InferVar::Float(var)))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn bool_does_not_set_has_ty_infer() {
    let frozen = test_frozen_ty_ctx();
    let flags = frozen.ty_flags(Ty::BOOL);
    assert!(!flags.contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn never_does_not_set_has_ty_infer() {
    let frozen = test_frozen_ty_ctx();
    let flags = frozen.ty_flags(Ty::NEVER);
    assert!(!flags.contains(TypeFlags::HAS_TY_INFER));
}

// --- HAS_ERROR ---

#[test]
fn error_ty_sets_has_error() {
    let frozen = test_frozen_ty_ctx();
    let flags = frozen.ty_flags(Ty::ERROR);
    assert!(flags.contains(TypeFlags::HAS_ERROR));
}

#[test]
fn ref_to_error_sets_has_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| c.mk_ref(Region::Erased, Ty::ERROR, Mutability::Not));
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn slice_of_error_sets_has_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Slice(Ty::ERROR)));
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn raw_ptr_to_error_sets_has_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::RawPtr(Ty::ERROR, Mutability::Not)));
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn adt_with_error_substs_sets_has_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let args = vec![GenericArg::Ty(Ty::ERROR)];
        let substs = c.intern_substitution(args);
        c.mk_adt(glyim_core::def_id::AdtId::from_raw(1), substs)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn tuple_with_error_sets_has_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let args = vec![GenericArg::Ty(Ty::ERROR)];
        let substs = c.intern_substitution(args);
        c.mk_tuple(substs)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn fn_ptr_with_error_input_sets_has_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let inputs = c.intern_substitution(vec![GenericArg::Ty(Ty::ERROR)]);
        let sig = FnSig {
            inputs,
            output: c.bool_ty(),
            c_variadic: false,
            unsafety: glyim_core::primitives::Safety::Safe,
            abi: glyim_core::primitives::Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn fn_ptr_with_error_output_sets_has_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let inputs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let sig = FnSig {
            inputs,
            output: Ty::ERROR,
            c_variadic: false,
            unsafety: glyim_core::primitives::Safety::Safe,
            abi: glyim_core::primitives::Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

// --- HAS_TY_PARAM ---

#[test]
fn param_ty_sets_has_ty_param() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let param = ParamTy { index: 0, name };
        c.mk_ty(TyKind::Param(param))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_PARAM));
}

#[test]
fn ref_to_param_sets_has_ty_param() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let param = ParamTy { index: 0, name };
        let param_ty = c.mk_ty(TyKind::Param(param));
        c.mk_ref(Region::Erased, param_ty, Mutability::Not)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_PARAM));
}

// --- HAS_RE_INFER ---

#[test]
fn ref_with_region_var_sets_has_re_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let vid = c.new_region_var(Region::Erased);
        c.mk_ref(Region::Var(vid), c.bool_ty(), Mutability::Not)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_RE_INFER));
}

#[test]
fn ref_with_erased_region_does_not_set_has_re_infer() {
    let (frozen, ty) =
        with_fresh_ty_ctx(|c| c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not));
    assert!(!frozen.ty_flags(ty).contains(TypeFlags::HAS_RE_INFER));
}

// --- HAS_RE_PARAM ---

#[test]
fn ref_with_early_bound_region_sets_has_re_param() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("'a");
        let early = EarlyBoundRegion { index: 0, name };
        c.mk_ref(Region::EarlyBound(early), c.bool_ty(), Mutability::Not)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_RE_PARAM));
}

#[test]
fn ref_with_static_region_does_not_set_has_re_param() {
    let (frozen, ty) =
        with_fresh_ty_ctx(|c| c.mk_ref(Region::Static, c.bool_ty(), Mutability::Not));
    assert!(!frozen.ty_flags(ty).contains(TypeFlags::HAS_RE_PARAM));
}

// --- Combinations ---

#[test]
fn ref_to_infer_with_region_var_sets_both_infer_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer_ty = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let vid = c.new_region_var(Region::Erased);
        c.mk_ref(Region::Var(vid), infer_ty, Mutability::Not)
    });
    let flags = frozen.ty_flags(ty);
    assert!(flags.contains(TypeFlags::HAS_TY_INFER));
    assert!(flags.contains(TypeFlags::HAS_RE_INFER));
}

#[test]
fn nested_ref_propagates_inner_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer_ty = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let inner_ref = c.mk_ref(Region::Erased, infer_ty, Mutability::Not);
        c.mk_ref(Region::Erased, inner_ref, Mutability::Mut)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn plain_types_have_no_flags() {
    let frozen = test_frozen_ty_ctx();
    for (ty, label) in [(Ty::BOOL, "bool"), (Ty::NEVER, "never"), (Ty::UNIT, "unit")] {
        let flags = frozen.ty_flags(ty);
        assert!(
            flags.is_empty(),
            "{label} should have no flags, got {flags:?}"
        );
    }
}

#[test]
fn int_types_have_no_flags() {
    let (frozen, i32_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    let (frozen2, u64_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U64)));
    assert!(frozen.ty_flags(i32_ty).is_empty());
    assert!(frozen2.ty_flags(u64_ty).is_empty());
}
