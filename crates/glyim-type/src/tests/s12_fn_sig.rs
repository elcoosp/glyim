//! S12-T01 extended: Tests for register_fn_sig, fn_sig, register_closure_sig,
//! closure_sig, register_body_ty, body_ty methods.

use glyim_core::def_id::{ClosureId, FnDefId, LocalDefId, TraitDefId};
use glyim_core::primitives::{Abi, IntTy, Safety, UintTy};

use super::helpers::{test_ty_ctx, with_fresh_ty_ctx};
use crate::fn_sig::FnSig;
use crate::substitution::GenericArg;
use crate::*;

// ---- FnSig registration and retrieval ----

#[test]
fn register_fn_sig_and_retrieve() {
    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let sig = FnSig {
        inputs,
        output: bool_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_id = FnDefId::from_raw(42);
    ctx.register_fn_sig(fn_id, sig.clone());
    let retrieved = ctx.fn_sig(fn_id);
    assert!(
        retrieved.is_some(),
        "fn_sig should return Some after registration"
    );
    let retrieved_sig = retrieved.unwrap();
    assert_eq!(retrieved_sig.output, bool_ty);
    assert_eq!(retrieved_sig.unsafety, Safety::Safe);
    assert!(!retrieved_sig.c_variadic);
}

#[test]
fn fn_sig_returns_none_for_unregistered() {
    let ctx = test_ty_ctx();
    let fn_id = FnDefId::from_raw(999);
    assert!(
        ctx.fn_sig(fn_id).is_none(),
        "fn_sig should return None for unregistered id"
    );
}

#[test]
fn register_multiple_fn_sigs() {
    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let u32_ty = ctx.mk_ty(TyKind::Uint(UintTy::U32));
    let inputs1 = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let inputs2 = ctx.intern_substitution(vec![GenericArg::Ty(u32_ty)]);
    let sig1 = FnSig {
        inputs: inputs1,
        output: ctx.bool_ty(),
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let sig2 = FnSig {
        inputs: inputs2,
        output: ctx.never_ty(),
        c_variadic: false,
        unsafety: Safety::Unsafe,
        abi: Abi::C,
    };
    let id1 = FnDefId::from_raw(1);
    let id2 = FnDefId::from_raw(2);
    ctx.register_fn_sig(id1, sig1);
    ctx.register_fn_sig(id2, sig2);
    assert_eq!(ctx.fn_sig(id1).unwrap().unsafety, Safety::Safe);
    assert_eq!(ctx.fn_sig(id2).unwrap().unsafety, Safety::Unsafe);
    assert_eq!(ctx.fn_sig(id2).unwrap().abi, Abi::C);
}

#[test]
fn fn_sig_survives_freeze() {
    let (frozen, _) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        let sig = FnSig {
            inputs,
            output: ctx.bool_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let fn_id = FnDefId::from_raw(7);
        ctx.register_fn_sig(fn_id, sig);
    });
    let fn_id = FnDefId::from_raw(7);
    assert!(
        frozen.fn_sig(fn_id).is_some(),
        "fn_sig should survive freeze"
    );
    assert_eq!(frozen.fn_sig(fn_id).unwrap().output, frozen.bool_ty());
}

#[test]
fn fn_sig_variadic() {
    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let sig = FnSig {
        inputs,
        output: ctx.unit_ty(),
        c_variadic: true,
        unsafety: Safety::Safe,
        abi: Abi::C,
    };
    let fn_id = FnDefId::from_raw(10);
    ctx.register_fn_sig(fn_id, sig);
    assert!(ctx.fn_sig(fn_id).unwrap().c_variadic);
}

// ---- ClosureSig registration and retrieval ----

#[test]
fn register_closure_sig_and_retrieve() {
    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let sig = FnSig {
        inputs,
        output: ctx.bool_ty(),
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let closure_id = ClosureId::from_raw(5);
    ctx.register_closure_sig(closure_id, sig.clone());
    let retrieved = ctx.closure_sig(closure_id);
    assert!(
        retrieved.is_some(),
        "closure_sig should return Some after registration"
    );
    assert_eq!(retrieved.unwrap().output, ctx.bool_ty());
}

#[test]
fn closure_sig_returns_none_for_unregistered() {
    let ctx = test_ty_ctx();
    let closure_id = ClosureId::from_raw(999);
    assert!(ctx.closure_sig(closure_id).is_none());
}

#[test]
fn closure_sig_survives_freeze() {
    let (frozen, _) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        let sig = FnSig {
            inputs,
            output: ctx.bool_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let closure_id = ClosureId::from_raw(3);
        ctx.register_closure_sig(closure_id, sig);
    });
    let closure_id = ClosureId::from_raw(3);
    assert!(frozen.closure_sig(closure_id).is_some());
}

// ---- BodyTy registration and retrieval ----

#[test]
fn register_body_ty_and_retrieve() {
    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let def_id = LocalDefId::from_raw(1);
    ctx.register_body_ty(def_id, i32_ty);
    assert_eq!(ctx.body_ty(def_id), Some(i32_ty));
}

#[test]
fn body_ty_returns_none_for_unregistered() {
    let ctx = test_ty_ctx();
    let def_id = LocalDefId::from_raw(999);
    assert!(ctx.body_ty(def_id).is_none());
}

#[test]
fn body_ty_survives_freeze() {
    let (frozen, bool_ty) = with_fresh_ty_ctx(|ctx| {
        let def_id = LocalDefId::from_raw(10);
        let ty = ctx.bool_ty();
        ctx.register_body_ty(def_id, ty);
        ty
    });
    let def_id = LocalDefId::from_raw(10);
    assert_eq!(frozen.body_ty(def_id), Some(bool_ty));
}

#[test]
fn register_body_ty_overwrites() {
    let mut ctx = test_ty_ctx();
    let def_id = LocalDefId::from_raw(1);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    ctx.register_body_ty(def_id, i32_ty);
    ctx.register_body_ty(def_id, bool_ty);
    assert_eq!(ctx.body_ty(def_id), Some(bool_ty));
}

// ---- P5 associated-type projection table ----

#[test]
fn impl_assoc_type_projection_resolves() {
    // Plan unstub-5 P5: an impl's `type Output = i32;` must be resolvable via
    // the projection table keyed by (self_ty, trait_def_id). Mirrors the
    // `impl MyFuture for AddOne { type Output = i32; }` desugar target.
    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let self_ty = ctx.mk_ty(TyKind::Int(IntTy::I64)); // stand-in for `AddOne`
    let trait_def_id = TraitDefId::from_raw(7);
    let output_name = ctx.resolver().intern("Output");

    ctx.register_impl_assoc_types(self_ty, trait_def_id, vec![(output_name, i32_ty)]);

    let resolved = ctx.resolve_associated_type(self_ty, trait_def_id, output_name);
    assert_eq!(resolved, Some(i32_ty));

    // Unknown associated type on a known impl → None.
    let other_name = ctx.resolver().intern("Other");
    assert_eq!(
        ctx.resolve_associated_type(self_ty, trait_def_id, other_name),
        None
    );

    // Wrong self type → None (projection is keyed by the concrete self type).
    let other_self = ctx.mk_ty(TyKind::Int(IntTy::I8));
    assert_eq!(
        ctx.resolve_associated_type(other_self, trait_def_id, output_name),
        None
    );
}

#[test]
fn impl_assoc_type_projection_survives_freeze() {
    // The projection table must survive `freeze` so the frozen `TyCtx` (used
    // by codegen) can resolve `Self::Output` too.
    let (frozen, (i32_ty, self_ty, trait_def_id, output_name)) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let self_ty = ctx.mk_ty(TyKind::Int(IntTy::I64));
        let trait_def_id = TraitDefId::from_raw(7);
        let output_name = ctx.resolver().intern("Output");
        ctx.register_impl_assoc_types(self_ty, trait_def_id, vec![(output_name, i32_ty)]);
        (i32_ty, self_ty, trait_def_id, output_name)
    });
    assert_eq!(
        frozen.resolve_associated_type(self_ty, trait_def_id, output_name),
        Some(i32_ty)
    );
}
