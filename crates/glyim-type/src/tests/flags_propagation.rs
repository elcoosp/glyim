//! Tests for `TypeFlags` propagation through all `TyKind` variants that delegate to children.

use glyim_core::def_id::{AdtId, ClosureId, FnDefId, OpaqueTyId};
use glyim_core::primitives::{Abi, IntTy, Mutability, Safety, UintTy};

use super::helpers::with_fresh_ty_ctx;
use crate::*;

// --- Adt flag propagation ---

#[test]
fn adt_with_infer_propagates_has_ty_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let substs = c.intern_substitution(vec![GenericArg::Ty(infer)]);
        c.mk_adt(AdtId::from_raw(1), substs)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn adt_with_param_propagates_has_ty_param() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let param = c.mk_ty(TyKind::Param(ParamTy { index: 0, name }));
        let substs = c.intern_substitution(vec![GenericArg::Ty(param)]);
        c.mk_adt(AdtId::from_raw(1), substs)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_PARAM));
}

#[test]
fn adt_with_error_propagates_has_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(Ty::ERROR)]);
        c.mk_adt(AdtId::from_raw(1), substs)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn adt_with_no_special_args_has_no_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let substs =
            c.intern_substitution(vec![GenericArg::Ty(c.bool_ty()), GenericArg::Ty(i32_ty)]);
        c.mk_adt(AdtId::from_raw(1), substs)
    });
    assert!(frozen.ty_flags(ty).is_empty());
}

// --- Tuple flag propagation ---

#[test]
fn tuple_with_infer_propagates() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let substs = c.intern_substitution(vec![GenericArg::Ty(infer)]);
        c.mk_tuple(substs)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn tuple_mixed_infer_and_error_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let substs = c.intern_substitution(vec![GenericArg::Ty(infer), GenericArg::Ty(Ty::ERROR)]);
        c.mk_tuple(substs)
    });
    let flags = frozen.ty_flags(ty);
    assert!(flags.contains(TypeFlags::HAS_TY_INFER));
    assert!(flags.contains(TypeFlags::HAS_ERROR));
}

// --- FnDef flag propagation ---

#[test]
fn fn_def_with_infer_propagates() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let substs = c.intern_substitution(vec![GenericArg::Ty(infer)]);
        c.mk_ty(TyKind::FnDef(FnDefId::from_raw(1), substs))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn fn_def_with_error_propagates() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(Ty::ERROR)]);
        c.mk_ty(TyKind::FnDef(FnDefId::from_raw(1), substs))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

// --- Closure flag propagation ---

#[test]
fn closure_with_infer_propagates() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let substs = c.intern_substitution(vec![GenericArg::Ty(infer)]);
        c.mk_ty(TyKind::Closure(ClosureId::from_raw(1), substs))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn closure_with_param_propagates() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let param = c.mk_ty(TyKind::Param(ParamTy { index: 0, name }));
        let substs = c.intern_substitution(vec![GenericArg::Ty(param)]);
        c.mk_ty(TyKind::Closure(ClosureId::from_raw(1), substs))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_PARAM));
}

// --- Opaque flag propagation ---

#[test]
fn opaque_with_infer_propagates() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let substs = c.intern_substitution(vec![GenericArg::Ty(infer)]);
        c.mk_ty(TyKind::Opaque(OpaqueTyId::from_raw(1), substs))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn opaque_with_error_propagates() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(Ty::ERROR)]);
        c.mk_ty(TyKind::Opaque(OpaqueTyId::from_raw(1), substs))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

// --- FnPtr flag propagation ---

#[test]
fn fn_ptr_propagates_input_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let inputs = c.intern_substitution(vec![GenericArg::Ty(infer)]);
        let sig = FnSig {
            inputs,
            output: c.bool_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn fn_ptr_propagates_output_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let inputs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let sig = FnSig {
            inputs,
            output: Ty::ERROR,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn fn_ptr_propagates_input_param() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let param = c.mk_ty(TyKind::Param(ParamTy { index: 0, name }));
        let inputs = c.intern_substitution(vec![GenericArg::Ty(param)]);
        let sig = FnSig {
            inputs,
            output: c.bool_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_PARAM));
}

// --- Ref flag propagation ---

#[test]
fn ref_propagates_inner_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| c.mk_ref(Region::Erased, Ty::ERROR, Mutability::Not));
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn ref_propagates_inner_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        c.mk_ref(Region::Erased, infer, Mutability::Not)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn ref_mut_with_region_var_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let vid = c.new_region_var(Region::Erased);
        c.mk_ref(Region::Var(vid), c.bool_ty(), Mutability::Mut)
    });
    let flags = frozen.ty_flags(ty);
    assert!(flags.contains(TypeFlags::HAS_RE_INFER));
    assert!(!flags.contains(TypeFlags::HAS_TY_INFER));
}

// --- RawPtr flag propagation ---

#[test]
fn raw_ptr_propagates_inner_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        c.mk_ty(TyKind::RawPtr(infer, Mutability::Not))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn raw_ptr_propagates_inner_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::RawPtr(Ty::ERROR, Mutability::Mut)));
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

// --- Slice flag propagation ---

#[test]
fn slice_propagates_inner_param() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let param = c.mk_ty(TyKind::Param(ParamTy { index: 0, name }));
        c.mk_ty(TyKind::Slice(param))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_PARAM));
}

// --- Array flag propagation ---

#[test]
fn array_propagates_inner_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let len = Const {
            kind: ConstKind::Uint(3),
            ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        c.mk_ty(TyKind::Array(infer, len))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn array_propagates_inner_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let len = Const {
            kind: ConstKind::Uint(3),
            ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        c.mk_ty(TyKind::Array(Ty::ERROR, len))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

// --- Combined flags ---

#[test]
fn adt_with_infer_and_error_combines_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let substs = c.intern_substitution(vec![GenericArg::Ty(infer), GenericArg::Ty(Ty::ERROR)]);
        c.mk_adt(AdtId::from_raw(1), substs)
    });
    let flags = frozen.ty_flags(ty);
    assert!(flags.contains(TypeFlags::HAS_TY_INFER));
    assert!(flags.contains(TypeFlags::HAS_ERROR));
}

#[test]
fn fn_ptr_with_all_infer_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let vid = c.new_region_var(Region::Erased);
        let inputs = c.intern_substitution(vec![GenericArg::Ty(infer)]);
        let sig = FnSig {
            inputs,
            output: infer,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let fn_ptr = c.mk_fn_ptr(sig);
        // Wrap in a ref with region var
        c.mk_ref(Region::Var(vid), fn_ptr, Mutability::Not)
    });
    let flags = frozen.ty_flags(ty);
    assert!(flags.contains(TypeFlags::HAS_TY_INFER));
    assert!(flags.contains(TypeFlags::HAS_RE_INFER));
}
