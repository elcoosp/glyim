//! Tests for associated type projections.
use glyim_core::def_id::TraitDefId;
use glyim_core::primitives::{Abi, IntTy, Mutability, Safety};

use crate::{
    FnSig, GenericArg, InferVar, ParamTy, PrintTy, ProjectionTy, Region, TraitRef, Ty, TyCtxMut,
    TyKind, TyVar, TypeFlags,
};

use super::test_helpers::with_fresh_ty_ctx;

/// Helper: build a projection type `<self_ty as trait_def_id>::item_name`
fn make_projection(
    ctx: &mut TyCtxMut,
    trait_def_id: TraitDefId,
    self_ty: crate::Ty,
    item_name: &str,
) -> crate::Ty {
    let name = ctx.resolver().intern(item_name);
    let substs = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
    let trait_ref = TraitRef {
        def_id: trait_def_id,
        substs,
    };
    let proj = ProjectionTy {
        trait_ref,
        item_name: name,
    };
    ctx.mk_ty(TyKind::Projection(proj))
}

#[test]
fn projection_type_construction() {
    let (ctx, proj_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(0);
        let self_ty = ctx.bool_ty();
        make_projection(ctx, trait_id, self_ty, "Item")
    });
    assert!(!ctx.ty_is_error(proj_ty));
    let kind = ctx.ty_kind(proj_ty);
    assert!(matches!(kind, TyKind::Projection(_)));
}

#[test]
fn projection_type_flags() {
    let (ctx, proj_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(0);
        let self_ty = ctx.bool_ty();
        make_projection(ctx, trait_id, self_ty, "Item")
    });
    let flags = ctx.ty_flags(proj_ty);
    assert!(!flags.contains(TypeFlags::HAS_TY_INFER));
    assert!(!flags.contains(TypeFlags::HAS_TY_PARAM));
}

#[test]
fn projection_type_flags_with_infer() {
    let (ctx, proj_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(0);
        let infer_var = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
        make_projection(ctx, trait_id, infer_var, "Item")
    });
    let flags = ctx.ty_flags(proj_ty);
    assert!(flags.contains(TypeFlags::HAS_TY_INFER));
}

#[test]
fn projection_display() {
    let (ctx, proj_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(0);
        let self_ty = ctx.bool_ty();
        make_projection(ctx, trait_id, self_ty, "Item")
    });
    let display = format!("{}", PrintTy::new(proj_ty, &ctx));
    assert!(display.contains("bool"));
    assert!(display.contains("Trait0"));
    assert!(display.contains("Item"));
}

#[test]
fn projection_equality() {
    let (ctx, (p1, p2, p3)) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(1);
        let self_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let item = ctx.resolver().intern("Item");
        let substs = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
        let trait_ref = TraitRef {
            def_id: trait_id,
            substs,
        };
        let proj1 = ctx.mk_ty(TyKind::Projection(ProjectionTy {
            trait_ref: trait_ref.clone(),
            item_name: item,
        }));
        let proj2 = ctx.mk_ty(TyKind::Projection(ProjectionTy {
            trait_ref: trait_ref.clone(),
            item_name: item,
        }));
        let proj3 = ctx.mk_ty(TyKind::Projection(ProjectionTy {
            trait_ref,
            item_name: ctx.resolver().intern("Other"),
        }));
        (proj1, proj2, proj3)
    });
    // Ty indices differ, but underlying TyKinds are equal
    assert_eq!(ctx.ty_kind(p1), ctx.ty_kind(p2));
    assert_ne!(ctx.ty_kind(p1), ctx.ty_kind(p3));
}

// =============================================================
// Additional edge case and regression tests
// =============================================================

/// Projection inside a Ref should propagate flags correctly.
#[test]
fn projection_inside_ref() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(7);
        let self_ty = ctx.bool_ty();
        let proj = make_projection(ctx, trait_id, self_ty, "Item");
        ctx.mk_ref(Region::Erased, proj, Mutability::Not)
    });
    let kind = ctx.ty_kind(ref_ty);
    assert!(matches!(kind, TyKind::Ref(_, _, _)));
    let flags = ctx.ty_flags(ref_ty);
    assert!(!flags.contains(TypeFlags::HAS_TY_INFER));
    assert!(!flags.contains(TypeFlags::HAS_ERROR));
}

/// Projection with a Param self type should propagate `HAS_TY_PARAM`.
#[test]
fn projection_with_param_self() {
    let (ctx, proj_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(0);
        let param_ty = ctx.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: ctx.resolver().intern("T"),
        }));
        make_projection(ctx, trait_id, param_ty, "Item")
    });
    let flags = ctx.ty_flags(proj_ty);
    assert!(flags.contains(TypeFlags::HAS_TY_PARAM));
}

/// Projection with an Error self type should propagate `HAS_ERROR`.
#[test]
fn projection_with_error_self() {
    let (ctx, proj_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(0);
        let error_ty = Ty::ERROR;
        make_projection(ctx, trait_id, error_ty, "Item")
    });
    let flags = ctx.ty_flags(proj_ty);
    assert!(flags.contains(TypeFlags::HAS_ERROR));
}

/// Projection equality: different trait IDs should be unequal.
#[test]
fn projection_different_trait_ids_unequal() {
    let (ctx, (p1, p2)) = with_fresh_ty_ctx(|ctx| {
        let self_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let item = ctx.resolver().intern("Item");
        let substs = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
        let tr1 = TraitRef {
            def_id: TraitDefId::from_raw(0),
            substs,
        };
        let substs2 = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
        let tr2 = TraitRef {
            def_id: TraitDefId::from_raw(1),
            substs: substs2,
        };
        let proj1 = ctx.mk_ty(TyKind::Projection(ProjectionTy {
            trait_ref: tr1,
            item_name: item,
        }));
        let proj2 = ctx.mk_ty(TyKind::Projection(ProjectionTy {
            trait_ref: tr2,
            item_name: item,
        }));
        (proj1, proj2)
    });
    assert_ne!(ctx.ty_kind(p1), ctx.ty_kind(p2));
}

/// Projection with empty substitution should still work.
#[test]
fn projection_empty_substitution() {
    let (ctx, proj_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(42);
        let substs = ctx.intern_substitution(vec![]);
        let trait_ref = TraitRef {
            def_id: trait_id,
            substs,
        };
        let proj = ProjectionTy {
            trait_ref,
            item_name: ctx.resolver().intern("Output"),
        };
        ctx.mk_ty(TyKind::Projection(proj))
    });
    assert!(!ctx.ty_is_error(proj_ty));
}

/// Projection substitution with multiple type arguments.
#[test]
fn projection_with_multiple_substs() {
    let (ctx, proj_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(5);
        let t1 = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let t2 = ctx.mk_ty(TyKind::Bool);
        let substs = ctx.intern_substitution(vec![GenericArg::Ty(t1), GenericArg::Ty(t2)]);
        let trait_ref = TraitRef {
            def_id: trait_id,
            substs,
        };
        let proj = ProjectionTy {
            trait_ref,
            item_name: ctx.resolver().intern("Item"),
        };
        ctx.mk_ty(TyKind::Projection(proj))
    });
    let kind = ctx.ty_kind(proj_ty);
    if let TyKind::Projection(proj) = kind {
        assert_eq!(ctx.substitution_args(proj.trait_ref.substs).len(), 2);
    } else {
        panic!("expected Projection");
    }
}

/// Printing a projection with multiple substitutions.
#[test]
fn projection_display_multi_subst() {
    let (ctx, proj_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(0);
        let t1 = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let t2 = ctx.mk_ty(TyKind::Bool);
        let substs = ctx.intern_substitution(vec![GenericArg::Ty(t1), GenericArg::Ty(t2)]);
        let trait_ref = TraitRef {
            def_id: trait_id,
            substs,
        };
        let proj = ProjectionTy {
            trait_ref,
            item_name: ctx.resolver().intern("Item"),
        };
        ctx.mk_ty(TyKind::Projection(proj))
    });
    let display = format!("{}", PrintTy::new(proj_ty, &ctx));
    assert!(display.contains('?'));
    assert!(display.contains("Trait0"));
}

/// A function that returns a projection type.
#[test]
fn fn_ptr_returning_projection() {
    let (ctx, fn_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(0);
        let self_ty = ctx.bool_ty();
        let proj = make_projection(ctx, trait_id, self_ty, "Item");
        let inputs = ctx.intern_substitution(vec![]);
        let sig = FnSig {
            inputs,
            output: proj,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        ctx.mk_fn_ptr(sig)
    });
    let kind = ctx.ty_kind(fn_ty);
    assert!(matches!(kind, TyKind::FnPtr(_)));
}

/// Nested projection via substitution (not yet recursive, just compositional).
#[test]
fn projection_in_tuple() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|ctx| {
        let trait_id = TraitDefId::from_raw(0);
        let proj = make_projection(ctx, trait_id, ctx.bool_ty(), "Item");
        let int32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let substs = ctx.intern_substitution(vec![GenericArg::Ty(proj), GenericArg::Ty(int32_ty)]);
        ctx.mk_tuple(substs)
    });
    let kind = ctx.ty_kind(tuple_ty);
    assert!(matches!(kind, TyKind::Tuple(_)));
    let flags = ctx.ty_flags(tuple_ty);
    assert!(!flags.contains(TypeFlags::HAS_ERROR));
}
