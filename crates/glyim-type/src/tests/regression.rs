//! Regression tests for bugs found and fixed during development.

use glyim_core::def_id::OpaqueTyId;
use glyim_core::primitives::IntTy;

use super::helpers::with_fresh_ty_ctx;
use crate::*;

// Bug: TyKind::Opaque was missing from the compute_flags match arm,
// so Opaque types with infer/error in their substs did not propagate flags.
// Fixed by adding TyKind::Opaque to the same arm as Adt/FnDef/Closure/Tuple.

#[test]
fn regression_opaque_with_infer_propagates_has_ty_infer() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let var = TyVar::from_usize(0);
        let infer = c.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let substs = c.intern_substitution(vec![GenericArg::Ty(infer)]);
        c.mk_ty(TyKind::Opaque(OpaqueTyId::from_usize(1), substs))
    });
    assert!(
        frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_INFER),
        "Opaque with infer substitution must propagate HAS_TY_INFER"
    );
}

#[test]
fn regression_opaque_with_error_propagates_has_error() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(Ty::ERROR)]);
        c.mk_ty(TyKind::Opaque(OpaqueTyId::from_usize(1), substs))
    });
    assert!(
        frozen.ty_flags(ty).contains(TypeFlags::HAS_ERROR),
        "Opaque with error substitution must propagate HAS_ERROR"
    );
}

#[test]
fn regression_opaque_with_param_propagates_has_ty_param() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let param = c.mk_ty(TyKind::Param(ParamTy { index: 0, name }));
        let substs = c.intern_substitution(vec![GenericArg::Ty(param)]);
        c.mk_ty(TyKind::Opaque(OpaqueTyId::from_usize(1), substs))
    });
    assert!(
        frozen.ty_flags(ty).contains(TypeFlags::HAS_TY_PARAM),
        "Opaque with param substitution must propagate HAS_TY_PARAM"
    );
}

// Bug: Name::default() doesn't exist — must use intern() to create Name values.
// This is now a documented pattern: always use ctx.resolver().intern() for names.

#[test]
fn regression_name_via_intern_not_default() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let param = ParamTy { index: 0, name };
        c.mk_ty(TyKind::Param(param))
    });
    assert!(matches!(frozen.ty_kind(ty), TyKind::Param(_)));
}

// Bug: Box::new([pred]) creates Box<[Predicate; 1]>, not Box<[Predicate]>.
// Must explicitly annotate: let preds: Box<[Predicate]> = Box::new([pred]);

#[test]
fn regression_dynamic_with_box_annotation() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let trait_substs = c.intern_substitution(vec![]);
        let pred = Predicate::Trait(TraitPredicate {
            trait_ref: TraitRef {
                def_id: glyim_core::def_id::TraitDefId::from_usize(1),
                substs: trait_substs,
            },
            polarity: ImplPolarity::Positive,
        });
        let preds: Box<[Predicate]> = Box::new([pred]);
        let binder = Binder::bind(preds, Box::new([BoundVariableKind::Ty(BoundTyKind::Anon)]));
        c.mk_ty(TyKind::Dynamic(binder, Region::Erased))
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::Dynamic(_, _)));
}

// Bug: Double mutable borrow when calling c.mk_ty() inside c.intern_substitution().
// Must pre-allocate types before passing to intern_substitution.

#[test]
fn regression_double_borrow_pattern() {
    let (ctx, sub) = with_fresh_ty_ctx(|c| {
        // This pattern used to cause double mutable borrow:
        // c.intern_substitution(vec![GenericArg::Ty(c.mk_ty(...))])
        // Correct pattern: pre-allocate
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let args = vec![GenericArg::Ty(c.bool_ty()), GenericArg::Ty(i32_ty)];
        c.intern_substitution(args)
    });
    assert_eq!(sub.len(), 2);
    let args = ctx.substitution_args(sub);
    assert_eq!(args.len(), 2);
}

// Bug: Sentinels must be at specific indices. This test ensures they stay stable.

#[test]
fn regression_sentinel_indices_stable() {
    assert_eq!(Ty::ERROR.to_raw(), 0, "Ty::ERROR must be index 0");
    assert_eq!(Ty::NEVER.to_raw(), 1, "Ty::NEVER must be index 1");
    assert_eq!(Ty::UNIT.to_raw(), 2, "Ty::UNIT must be index 2");
    assert_eq!(Ty::BOOL.to_raw(), 3, "Ty::BOOL must be index 3");
}

// Bug: Substitution::from_raw is pub(crate) — external code must use intern_substitution.
// Ensure the API is used correctly.

#[test]
fn regression_substitution_construction_via_context() {
    let (ctx, sub) = with_fresh_ty_ctx(|c| {
        c.intern_substitution(vec![
            GenericArg::Ty(c.bool_ty()),
            GenericArg::Lifetime(Region::Erased),
        ])
    });
    assert_eq!(sub.len(), 2);
    let args = ctx.substitution_args(sub);
    assert!(matches!(&args[0], GenericArg::Ty(t) if *t == Ty::BOOL));
    assert!(matches!(&args[1], GenericArg::Lifetime(Region::Erased)));
}
