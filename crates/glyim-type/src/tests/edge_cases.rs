//! Edge case tests: large indices, many types, `fn_def/closure` flags propagation.

use glyim_core::def_id::{AdtId, ClosureId, FnDefId};
use glyim_core::primitives::{Abi, IntTy, Mutability, Safety, UintTy};

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn many_types_allocated() {
    let (frozen, tys) = with_fresh_ty_ctx(|c| {
        let mut tys = Vec::new();
        for _ in 0..100u32 {
            let ty = c.mk_ty(TyKind::Int(IntTy::I32));
            tys.push(ty);
        }
        tys
    });
    assert_eq!(tys.len(), 100);
    // Canonical interner: every `i32` resolves to the same shared handle.
    let first = tys[0];
    for ty in &tys {
        assert_eq!(*ty, first);
        assert!(matches!(frozen.ty_kind(*ty), TyKind::Int(IntTy::I32)));
    }
}

#[test]
fn large_adt_id() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let adt_id = AdtId::from_raw(u32::MAX);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(adt_id, substs)
    });
    if let TyKind::Adt(id, substs) = ctx.ty_kind(ty) {
        assert_eq!(id.to_raw(), u32::MAX);
        assert!(substs.is_empty());
    } else {
        panic!("expected Adt");
    }
}

#[test]
fn large_fn_def_id() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let fn_def_id = FnDefId::from_raw(u32::MAX / 2);
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::FnDef(fn_def_id, substs))
    });
    if let TyKind::FnDef(id, substs) = ctx.ty_kind(ty) {
        assert_eq!(id.to_raw(), u32::MAX / 2);
        assert!(substs.is_empty());
    } else {
        panic!("expected FnDef");
    }
}

#[test]
fn fn_def_with_infer_propagates_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer_ty = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let substs = c.intern_substitution(vec![GenericArg::Ty(infer_ty)]);
        c.mk_ty(TyKind::FnDef(FnDefId::from_raw(1), substs))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn closure_with_error_propagates_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(Ty::ERROR)]);
        c.mk_ty(TyKind::Closure(ClosureId::from_raw(1), substs))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn fn_ptr_with_infer_input_propagates_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer_ty = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let inputs = c.intern_substitution(vec![GenericArg::Ty(infer_ty)]);
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
fn fn_ptr_with_infer_output_propagates_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer_ty = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let inputs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let sig = FnSig {
            inputs,
            output: infer_ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn array_with_infer_elem_propagates_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_raw(0);
        let infer_ty = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let len = Const {
            kind: ConstKind::Uint(3),
            ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        c.mk_ty(TyKind::Array(infer_ty, len))
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn slice_with_error_propagates_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Slice(Ty::ERROR)));
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR));
}

#[test]
fn deep_nesting_ref() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let mut current = c.bool_ty();
        for _ in 0..20 {
            current = c.mk_ref(Region::Erased, current, Mutability::Not);
        }
        current
    });
    // Should be &bool after 20 levels
    let flags = frozen.ty_flags(ty);
    assert!(!flags.contains(TypeFlags::HAS_ERROR));
    assert!(!flags.contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn tuple_with_many_infer_args() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let args: Vec<GenericArg> = (0..10)
            .map(|i| {
                let var = TyVar::from_raw(i);
                GenericArg::Ty(c.mk_ty(TyKind::Infer(InferVar::Ty(var))))
            })
            .collect();
        let substs = c.intern_substitution(args);
        c.mk_tuple(substs)
    });
    assert!(frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn ty_from_raw_and_to_raw_roundtrip() {
    for raw in [0u32, 1, 2, 3, 4, 100, u32::MAX] {
        let ty = Ty::from_raw(raw);
        assert_eq!(ty.to_raw(), raw);
        assert_eq!(ty.index(), raw as usize);
    }
}

#[test]
fn universe_index() {
    let u0 = UniverseIndex(0);
    let u1 = UniverseIndex(1);
    assert_eq!(u0.0, 0);
    assert_eq!(u1.0, 1);
}

#[test]
fn field_idx() {
    let f0 = FieldIdx::from_raw(0);
    let f5 = FieldIdx::from_raw(5);
    assert_eq!(f0.to_raw(), 0);
    assert_eq!(f5.to_raw(), 5);
}

#[test]
fn const_var() {
    let cv = ConstVar::from_raw(42);
    assert_eq!(cv.to_raw(), 42);
}

#[test]
fn region_vid_sequential() {
    let v0 = RegionVid::from_raw(0);
    let v1 = RegionVid::from_raw(1);
    assert_eq!(v0.to_raw(), 0);
    assert_eq!(v1.to_raw(), 1);
}

#[test]
fn sentinel_indices_are_stable() {
    // These must be stable across all compilations
    assert_eq!(Ty::ERROR.to_raw(), 0);
    assert_eq!(Ty::NEVER.to_raw(), 1);
    assert_eq!(Ty::UNIT.to_raw(), 2);
    assert_eq!(Ty::BOOL.to_raw(), 3);
}

#[test]
fn error_region_in_ref_no_special_flags() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| c.mk_ref(Region::Error, c.bool_ty(), Mutability::Not));
    let flags = frozen.ty_flags(ty);
    // Region::Error is neither Var nor EarlyBound, so no RE_INFER/RE_PARAM
    assert!(!flags.contains(TypeFlags::HAS_RE_INFER));
    assert!(!flags.contains(TypeFlags::HAS_RE_PARAM));
}

#[test]
fn ty_debug_format_custom() {
    let ty = Ty::from_raw(999);
    let debug = format!("{ty:?}");
    assert_eq!(debug, "Ty(999)");
}
