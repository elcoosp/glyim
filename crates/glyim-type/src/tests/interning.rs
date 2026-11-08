use glyim_core::def_id::AdtId;
use glyim_core::primitives::{Abi, FloatTy, IntTy, Mutability, Safety, UintTy};

use super::helpers::{test_frozen_ty_ctx, with_fresh_ty_ctx};
use crate::*;

// S02-T01: Sentinel constants at correct indices

#[test]
fn sentinel_error_at_index_0() {
    assert_eq!(Ty::ERROR.to_raw(), 0);
}

#[test]
fn sentinel_never_at_index_1() {
    assert_eq!(Ty::NEVER.to_raw(), 1);
}

#[test]
fn sentinel_unit_at_index_2() {
    assert_eq!(Ty::UNIT.to_raw(), 2);
}

#[test]
fn sentinel_bool_at_index_3() {
    assert_eq!(Ty::BOOL.to_raw(), 3);
}

#[test]
fn sentinels_roundtrip_via_ty_kind() {
    let ctx = test_frozen_ty_ctx();
    assert!(matches!(ctx.ty_kind(Ty::ERROR), TyKind::Error));
    assert!(matches!(ctx.ty_kind(Ty::NEVER), TyKind::Never));
    assert!(matches!(ctx.ty_kind(Ty::UNIT), TyKind::Unit));
    assert!(matches!(ctx.ty_kind(Ty::BOOL), TyKind::Bool));
}

// S02-T03: mk_ref creates Ref; roundtrip via ty_kind

#[test]
fn mk_ref_creates_ref_roundtrip() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        c.mk_ref(Region::Erased, inner, Mutability::Mut)
    });
    match ctx.ty_kind(ref_ty) {
        TyKind::Ref(region, inner, mutability) => {
            assert!(matches!(region, Region::Erased));
            assert_eq!(*inner, Ty::BOOL);
            assert_eq!(*mutability, Mutability::Mut);
        }
        other => panic!("expected Ref, got {other:?}"),
    }
}

// S02-T04: mk_ref with Not mutability

#[test]
fn mk_ref_with_not_mutability() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.unit_ty();
        c.mk_ref(Region::Erased, inner, Mutability::Not)
    });
    match ctx.ty_kind(ref_ty) {
        TyKind::Ref(region, inner, mutability) => {
            assert!(matches!(region, Region::Erased));
            assert_eq!(*inner, Ty::UNIT);
            assert_eq!(*mutability, Mutability::Not);
        }
        other => panic!("expected Ref, got {other:?}"),
    }
}

// S02-T05: mk_adt with empty Substitution

#[test]
fn mk_adt_with_empty_substitution() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|c| {
        let adt_id = AdtId::from_raw(42);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(adt_id, substs)
    });
    match ctx.ty_kind(adt_ty) {
        TyKind::Adt(adt_id, substs) => {
            assert_eq!(adt_id.to_raw(), 42);
            assert!(substs.is_empty());
        }
        other => panic!("expected Adt, got {other:?}"),
    }
}

// S02-T06: mk_adt with Substitution containing Ty args

#[test]
fn mk_adt_with_substitution_containing_ty_args() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|c| {
        let adt_id = AdtId::from_raw(7);
        let bool_arg = GenericArg::Ty(c.bool_ty());
        let i32_arg = GenericArg::Ty(c.mk_ty(TyKind::Int(IntTy::I32)));
        let substs = c.intern_substitution(vec![bool_arg, i32_arg]);
        c.mk_adt(adt_id, substs)
    });
    match ctx.ty_kind(adt_ty) {
        TyKind::Adt(adt_id, substs) => {
            assert_eq!(adt_id.to_raw(), 7);
            assert_eq!(substs.len(), 2);
            let args = ctx.substitution_args(*substs);
            assert!(matches!(&args[0], GenericArg::Ty(t) if *t == Ty::BOOL));
            if let GenericArg::Ty(t) = &args[1] {
                assert!(matches!(ctx.ty_kind(*t), TyKind::Int(IntTy::I32)));
            } else {
                panic!("expected Ty arg at index 1");
            }
        }
        other => panic!("expected Adt, got {other:?}"),
    }
}

// S02-T07: mk_tuple with multiple tys

#[test]
fn mk_tuple_with_multiple_tys() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c| {
        let args = vec![
            GenericArg::Ty(c.bool_ty()),
            GenericArg::Ty(c.mk_ty(TyKind::Int(IntTy::I32))),
            GenericArg::Ty(c.mk_ty(TyKind::Uint(UintTy::U64))),
        ];
        let substs = c.intern_substitution(args);
        c.mk_tuple(substs)
    });
    match ctx.ty_kind(tuple_ty) {
        TyKind::Tuple(substs) => {
            assert_eq!(substs.len(), 3);
            let args = ctx.substitution_args(*substs);
            assert!(matches!(&args[0], GenericArg::Ty(t) if *t == Ty::BOOL));
        }
        other => panic!("expected Tuple, got {other:?}"),
    }
}

// S02-T08: mk_fn_ptr with FnSig

#[test]
fn mk_fn_ptr_with_fn_sig() {
    let (ctx, fn_ptr_ty) = with_fresh_ty_ctx(|c| {
        let input_args = vec![GenericArg::Ty(c.bool_ty())];
        let inputs = c.intern_substitution(input_args);
        let output = c.mk_ty(TyKind::Int(IntTy::I32));
        let sig = FnSig {
            inputs,
            output,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    match ctx.ty_kind(fn_ptr_ty) {
        TyKind::FnPtr(sig) => {
            assert!(!sig.c_variadic);
            assert_eq!(sig.unsafety, Safety::Safe);
            assert_eq!(sig.abi, Abi::Glyim);
            let input_args = ctx.substitution_args(sig.inputs);
            assert_eq!(input_args.len(), 1);
            assert!(matches!(&input_args[0], GenericArg::Ty(t) if *t == Ty::BOOL));
        }
        other => panic!("expected FnPtr, got {other:?}"),
    }
}

// S02-T11: error_ty() returns Ty::ERROR

#[test]
fn error_ty_returns_sentinel() {
    let ctx = test_frozen_ty_ctx();
    assert_eq!(ctx.error_ty(), Ty::ERROR);
    assert_eq!(ctx.error_ty().to_raw(), 0);
}

#[test]
fn never_ty_returns_sentinel() {
    let ctx = test_frozen_ty_ctx();
    assert_eq!(ctx.never_ty(), Ty::NEVER);
}

#[test]
fn unit_ty_returns_sentinel() {
    let ctx = test_frozen_ty_ctx();
    assert_eq!(ctx.unit_ty(), Ty::UNIT);
}

#[test]
fn bool_ty_returns_sentinel() {
    let ctx = test_frozen_ty_ctx();
    assert_eq!(ctx.bool_ty(), Ty::BOOL);
}

// Additional: mk_ty with various TyKinds

#[test]
fn mk_int_ty() {
    let (ctx, int_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    assert!(matches!(ctx.ty_kind(int_ty), TyKind::Int(IntTy::I32)));
}

#[test]
fn mk_uint_ty() {
    let (ctx, uint_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U64)));
    assert!(matches!(ctx.ty_kind(uint_ty), TyKind::Uint(UintTy::U64)));
}

#[test]
fn mk_float_ty() {
    let (ctx, float_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Float(FloatTy::F64)));
    assert!(matches!(ctx.ty_kind(float_ty), TyKind::Float(FloatTy::F64)));
}

#[test]
fn mk_slice_ty() {
    let (ctx, slice_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        c.mk_ty(TyKind::Slice(inner))
    });
    match ctx.ty_kind(slice_ty) {
        TyKind::Slice(inner) => assert_eq!(*inner, Ty::BOOL),
        other => panic!("expected Slice, got {other:?}"),
    }
}

#[test]
fn mk_raw_ptr_ty() {
    let (ctx, ptr_ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        c.mk_ty(TyKind::RawPtr(inner, Mutability::Not))
    });
    match ctx.ty_kind(ptr_ty) {
        TyKind::RawPtr(inner, mutability) => {
            assert_eq!(*inner, Ty::BOOL);
            assert_eq!(*mutability, Mutability::Not);
        }
        other => panic!("expected RawPtr, got {other:?}"),
    }
}
