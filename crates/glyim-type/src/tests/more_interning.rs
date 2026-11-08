//! Additional interning tests for less common `TyKind` variants.

use glyim_core::def_id::{ClosureId, FnDefId, OpaqueTyId};
use glyim_core::primitives::{Abi, IntTy, Mutability, Safety, UintTy};

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn mk_char_ty() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Char));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Char));
}

#[test]
fn mk_string_ty() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::String));
    assert!(matches!(ctx.ty_kind(ty), TyKind::String));
}

#[test]
fn mk_array_ty() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        let len = Const {
            kind: ConstKind::Uint(5),
            ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        c.mk_ty(TyKind::Array(inner, len))
    });
    match ctx.ty_kind(ty) {
        TyKind::Array(inner, len_const) => {
            assert_eq!(*inner, Ty::BOOL);
            assert!(matches!(len_const.kind, ConstKind::Uint(5)));
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn mk_fn_def_ty() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let fn_def_id = FnDefId::from_raw(10);
        let substs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        c.mk_ty(TyKind::FnDef(fn_def_id, substs))
    });
    match ctx.ty_kind(ty) {
        TyKind::FnDef(fn_def_id, substs) => {
            assert_eq!(fn_def_id.to_raw(), 10);
            assert_eq!(substs.len(), 1);
        }
        other => panic!("expected FnDef, got {other:?}"),
    }
}

#[test]
fn mk_closure_ty() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let closure_id = ClosureId::from_raw(20);
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::Closure(closure_id, substs))
    });
    match ctx.ty_kind(ty) {
        TyKind::Closure(closure_id, substs) => {
            assert_eq!(closure_id.to_raw(), 20);
            assert!(substs.is_empty());
        }
        other => panic!("expected Closure, got {other:?}"),
    }
}

#[test]
fn mk_opaque_ty() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let opaque_id = OpaqueTyId::from_raw(30);
        let inner_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(inner_ty)]);
        c.mk_ty(TyKind::Opaque(opaque_id, substs))
    });
    match ctx.ty_kind(ty) {
        TyKind::Opaque(opaque_id, substs) => {
            assert_eq!(opaque_id.to_raw(), 30);
            assert_eq!(substs.len(), 1);
        }
        other => panic!("expected Opaque, got {other:?}"),
    }
}

#[test]
fn mk_dynamic_ty() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let trait_substs = c.intern_substitution(vec![]);
        let pred = Predicate::Trait(TraitPredicate {
            trait_ref: TraitRef {
                def_id: glyim_core::def_id::TraitDefId::from_raw(1),
                substs: trait_substs,
            },
            polarity: ImplPolarity::Positive,
        });
        let preds: Box<[Predicate]> = Box::new([pred]);
        let binder = Binder::bind(preds, Box::new([BoundVariableKind::Ty(BoundTyKind::Anon)]));
        c.mk_ty(TyKind::Dynamic(binder, Region::Erased))
    });
    match ctx.ty_kind(ty) {
        TyKind::Dynamic(binder, region) => {
            assert!(matches!(region, Region::Erased));
            assert_eq!(binder.bound_vars.len(), 1);
        }
        other => panic!("expected Dynamic, got {other:?}"),
    }
}

#[test]
fn mk_param_ty() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let param = ParamTy { index: 0, name };
        c.mk_ty(TyKind::Param(param))
    });
    match ctx.ty_kind(ty) {
        TyKind::Param(p) => {
            assert_eq!(p.index, 0);
        }
        other => panic!("expected Param, got {other:?}"),
    }
}

#[test]
fn mk_bound_ty() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let bound = BoundTy {
            var: 0,
            kind: BoundTyKind::Anon,
        };
        c.mk_ty(TyKind::Bound(0, bound))
    });
    match ctx.ty_kind(ty) {
        TyKind::Bound(debruijn, bound) => {
            assert_eq!(*debruijn, 0);
            assert!(matches!(bound.kind, BoundTyKind::Anon));
        }
        other => panic!("expected Bound, got {other:?}"),
    }
}

#[test]
fn mk_fn_ptr_variadic() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inputs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let sig = FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: true,
            unsafety: Safety::Unsafe,
            abi: Abi::C,
        };
        c.mk_fn_ptr(sig)
    });
    match ctx.ty_kind(ty) {
        TyKind::FnPtr(sig) => {
            assert!(sig.c_variadic);
            assert_eq!(sig.unsafety, Safety::Unsafe);
            assert_eq!(sig.abi, Abi::C);
        }
        other => panic!("expected FnPtr, got {other:?}"),
    }
}

// --- mk_raw_ptr mut ---

#[test]
fn mk_raw_ptr_mut() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inner = c.bool_ty();
        c.mk_ty(TyKind::RawPtr(inner, Mutability::Mut))
    });
    match ctx.ty_kind(ty) {
        TyKind::RawPtr(inner, mutability) => {
            assert_eq!(*inner, Ty::BOOL);
            assert_eq!(*mutability, Mutability::Mut);
        }
        other => panic!("expected RawPtr, got {other:?}"),
    }
}
