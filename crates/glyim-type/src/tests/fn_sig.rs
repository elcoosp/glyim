//! Tests for `FnSig` construction and field access.

use glyim_core::primitives::{Abi, IntTy, Safety, UintTy};

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn fn_sig_safe_glyim() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inputs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let sig = FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    if let TyKind::FnPtr(sig) = ctx.ty_kind(ty) {
        assert_eq!(sig.unsafety, Safety::Safe);
        assert_eq!(sig.abi, Abi::Glyim);
        assert!(!sig.c_variadic);
        assert_eq!(ctx.substitution_args(sig.inputs).len(), 1);
        assert_eq!(sig.output, Ty::UNIT);
    } else {
        panic!("expected FnPtr");
    }
}

#[test]
fn fn_sig_unsafe_c() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u64_ty = c.mk_ty(TyKind::Uint(UintTy::U64));
        let inputs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(u64_ty)]);
        let output = c.mk_ty(TyKind::Int(IntTy::I32));
        let sig = FnSig {
            inputs,
            output,
            c_variadic: false,
            unsafety: Safety::Unsafe,
            abi: Abi::C,
        };
        c.mk_fn_ptr(sig)
    });
    if let TyKind::FnPtr(sig) = ctx.ty_kind(ty) {
        assert_eq!(sig.unsafety, Safety::Unsafe);
        assert_eq!(sig.abi, Abi::C);
        assert_eq!(ctx.substitution_args(sig.inputs).len(), 2);
    } else {
        panic!("expected FnPtr");
    }
}

#[test]
fn fn_sig_variadic_system() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inputs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let sig = FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: true,
            unsafety: Safety::Safe,
            abi: Abi::System,
        };
        c.mk_fn_ptr(sig)
    });
    if let TyKind::FnPtr(sig) = ctx.ty_kind(ty) {
        assert!(sig.c_variadic);
        assert_eq!(sig.abi, Abi::System);
    } else {
        panic!("expected FnPtr");
    }
}

#[test]
fn fn_sig_no_inputs() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inputs = c.intern_substitution(vec![]);
        let sig = FnSig {
            inputs,
            output: c.bool_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    if let TyKind::FnPtr(sig) = ctx.ty_kind(ty) {
        assert!(sig.inputs.is_empty());
        assert_eq!(sig.output, Ty::BOOL);
    } else {
        panic!("expected FnPtr");
    }
}

#[test]
fn fn_sig_equality() {
    let (ctx, (ty1, ty2)) = with_fresh_ty_ctx(|c| {
        let inputs1 = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let sig1 = FnSig {
            inputs: inputs1,
            output: c.unit_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let inputs2 = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let sig2 = FnSig {
            inputs: inputs2,
            output: c.unit_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        (c.mk_fn_ptr(sig1), c.mk_fn_ptr(sig2))
    });
    assert_eq!(ty1, ty2);
    assert!(matches!(ctx.ty_kind(ty1), TyKind::FnPtr(_)));
    assert!(matches!(ctx.ty_kind(ty2), TyKind::FnPtr(_)));
}

#[test]
fn fn_sig_debug_format() {
    let sig = FnSig {
        inputs: Substitution::from_raw(0, 1),
        output: Ty::BOOL,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let debug = format!("{sig:?}");
    assert!(debug.contains("FnSig"));
    assert!(debug.contains("c_variadic"));
    assert!(debug.contains("unsafety"));
    assert!(debug.contains("abi"));
}

#[test]
fn fn_sig_clone() {
    let sig = FnSig {
        inputs: Substitution::from_raw(0, 0),
        output: Ty::UNIT,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let cloned = sig.clone();
    assert_eq!(sig.output, cloned.output);
    assert_eq!(sig.c_variadic, cloned.c_variadic);
    assert_eq!(sig.unsafety, cloned.unsafety);
    assert_eq!(sig.abi, cloned.abi);
}
