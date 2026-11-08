//! Stress tests: large-scale type and substitution allocation.

use glyim_core::def_id::AdtId;
use glyim_core::primitives::{Abi, IntTy, Mutability, Safety, UintTy};

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn allocate_1000_types() {
    let (frozen, (count, last)) = with_fresh_ty_ctx(|c| {
        let mut count = 0usize;
        let mut last = c.bool_ty();
        for _ in 0..1000u32 {
            let ty = c.mk_ty(TyKind::Int(IntTy::I32));
            last = ty;
            count += 1;
        }
        (count, last)
    });
    assert_eq!(count, 1000);
    // All `i32` handles intern to the same shared handle; it must still
    // resolve correctly through the frozen context.
    assert!(matches!(frozen.ty_kind(last), TyKind::Int(IntTy::I32)));
}

#[test]
fn allocate_500_refs() {
    let (frozen, pair) = with_fresh_ty_ctx(|c| {
        let first = c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not);
        let mut current = first;
        for _ in 1..500 {
            current = c.mk_ref(Region::Erased, current, Mutability::Not);
        }
        (first, current)
    });
    let (first, last) = pair;
    assert!(matches!(frozen.ty_kind(first), TyKind::Ref(_, _, _)));
    assert!(matches!(frozen.ty_kind(last), TyKind::Ref(_, _, _)));
}

#[test]
fn allocate_100_substitutions() {
    let (frozen, subs) = with_fresh_ty_ctx(|c| {
        let mut subs = Vec::new();
        for i in 0..100u128 {
            let ty = c.mk_ty(TyKind::Uint(UintTy::U64));
            let cnst = Const {
                kind: ConstKind::Uint(i),
                ty,
            };
            let sub = c.intern_substitution(vec![GenericArg::Ty(ty), GenericArg::Const(cnst)]);
            subs.push(sub);
        }
        subs
    });
    assert_eq!(subs.len(), 100);
    for sub in &subs {
        assert_eq!(sub.len(), 2);
        let args = frozen.substitution_args(*sub);
        assert_eq!(args.len(), 2);
    }
}

#[test]
fn substitution_with_50_args() {
    let (frozen, sub) = with_fresh_ty_ctx(|c| {
        let mut args = Vec::with_capacity(50);
        for _ in 0..50 {
            args.push(GenericArg::Ty(c.bool_ty()));
        }
        c.intern_substitution(args)
    });
    assert_eq!(sub.len(), 50);
    let args = frozen.substitution_args(sub);
    assert_eq!(args.len(), 50);
    for arg in args {
        assert!(matches!(arg, GenericArg::Ty(t) if *t == Ty::BOOL));
    }
}

#[test]
fn many_adts_with_substs() {
    let (frozen, tys) = with_fresh_ty_ctx(|c| {
        let mut tys = Vec::new();
        for i in 0..200u32 {
            let inner = c.mk_ty(TyKind::Int(IntTy::I32));
            let substs = c.intern_substitution(vec![GenericArg::Ty(inner)]);
            let adt = c.mk_adt(AdtId::from_raw(i), substs);
            tys.push(adt);
        }
        tys
    });
    assert_eq!(tys.len(), 200);
    for (i, ty) in tys.iter().enumerate() {
        if let TyKind::Adt(id, substs) = frozen.ty_kind(*ty) {
            assert_eq!(id.to_raw(), i as u32);
            assert_eq!(substs.len(), 1);
        } else {
            panic!("expected Adt at index {i}");
        }
    }
}

#[test]
fn many_fn_ptrs() {
    let (frozen, tys) = with_fresh_ty_ctx(|c| {
        let mut tys = Vec::new();
        for _ in 0..100 {
            let inputs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
            let sig = FnSig {
                inputs,
                output: c.unit_ty(),
                c_variadic: false,
                unsafety: Safety::Safe,
                abi: Abi::Glyim,
            };
            tys.push(c.mk_fn_ptr(sig));
        }
        tys
    });
    assert_eq!(tys.len(), 100);
    for ty in &tys {
        assert!(matches!(frozen.ty_kind(*ty), TyKind::FnPtr(_)));
    }
}

#[test]
fn mixed_type_allocation_stress() {
    let (frozen, (count, last)) = with_fresh_ty_ctx(|c| {
        let mut count = 0usize;
        let mut last = c.bool_ty(); // placeholder
        for i in 0..100u32 {
            let _ = c.mk_ty(TyKind::Int(IntTy::I32));
            let _ = c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not);
            let _ = c.mk_ty(TyKind::Slice(c.bool_ty()));
            let inner = c.mk_ty(TyKind::Uint(UintTy::U64));
            let substs = c.intern_substitution(vec![GenericArg::Ty(inner)]);
            last = c.mk_adt(AdtId::from_raw(i), substs);
            count += 5;
        }
        (count, last)
    });
    assert_eq!(count, 500);
    assert!(matches!(frozen.ty_kind(last), TyKind::Adt(_, _)));
}
