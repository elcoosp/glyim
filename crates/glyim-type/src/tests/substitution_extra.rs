//! Additional substitution tests - ordering, mixed args, debug format.

use glyim_core::primitives::{IntTy, UintTy};

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn substitution_preserves_ordering() {
    let (ctx, sub) = with_fresh_ty_ctx(|c| {
        let t0 = c.mk_ty(TyKind::Int(IntTy::I32));
        let t2 = c.mk_ty(TyKind::Uint(UintTy::U64));
        let args = vec![
            GenericArg::Ty(t0),
            GenericArg::Ty(c.bool_ty()),
            GenericArg::Ty(t2),
        ];
        c.intern_substitution(args)
    });
    let args = ctx.substitution_args(sub);
    assert!(
        matches!(&args[0], GenericArg::Ty(t) if matches!(ctx.ty_kind(*t), TyKind::Int(IntTy::I32)))
    );
    assert!(matches!(&args[1], GenericArg::Ty(t) if *t == Ty::BOOL));
    assert!(
        matches!(&args[2], GenericArg::Ty(t) if matches!(ctx.ty_kind(*t), TyKind::Uint(UintTy::U64)))
    );
}

#[test]
fn substitution_mixed_args() {
    let (ctx, sub) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let args = vec![
            GenericArg::Ty(c.bool_ty()),
            GenericArg::Lifetime(Region::Static),
            GenericArg::Const(Const {
                kind: ConstKind::Uint(10),
                ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
            }),
            GenericArg::Ty(i32_ty),
        ];
        c.intern_substitution(args)
    });
    let args = ctx.substitution_args(sub);
    assert_eq!(args.len(), 4);
    assert!(matches!(&args[0], GenericArg::Ty(_)));
    assert!(matches!(&args[1], GenericArg::Lifetime(Region::Static)));
    assert!(matches!(&args[2], GenericArg::Const(_)));
    assert!(matches!(&args[3], GenericArg::Ty(_)));
}

#[test]
fn substitution_debug_format() {
    let sub = Substitution::from_raw(5, 3);
    let debug = format!("{sub:?}");
    assert!(debug.contains("index=5"));
    assert!(debug.contains("len=3"));
}

#[test]
fn substitution_len_vs_is_empty() {
    let (_ctx, empty) = with_fresh_ty_ctx(|c| c.intern_substitution(vec![]));
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);

    let (_ctx2, nonempty) =
        with_fresh_ty_ctx(|c| c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]));
    assert!(!nonempty.is_empty());
    assert_eq!(nonempty.len(), 1);
}

#[test]
fn many_substitutions_dont_interfere() {
    let (ctx, subs) = with_fresh_ty_ctx(|c| {
        let s0 = c.intern_substitution(vec![]);
        let s1 = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let s2 = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty()), GenericArg::Ty(i32_ty)]);
        let s3 = c.intern_substitution(vec![]);
        vec![s0, s1, s2, s3]
    });
    assert_eq!(subs[0].index(), subs[3].index());
    assert_ne!(subs[0].index(), subs[1].index());
    assert_ne!(subs[1].index(), subs[2].index());
    assert_eq!(ctx.substitution_args(subs[0]).len(), 0);
    assert_eq!(ctx.substitution_args(subs[1]).len(), 1);
    assert_eq!(ctx.substitution_args(subs[2]).len(), 2);
}

#[test]
fn substitution_with_single_lifetime() {
    let (ctx, sub) =
        with_fresh_ty_ctx(|c| c.intern_substitution(vec![GenericArg::Lifetime(Region::Static)]));
    let args = ctx.substitution_args(sub);
    assert_eq!(args.len(), 1);
    assert!(matches!(&args[0], GenericArg::Lifetime(Region::Static)));
}
