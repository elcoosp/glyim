use crate::InferenceTable;
use glyim_core::def_id::TraitDefId;
use glyim_core::interner::Interner;
use glyim_core::primitives::IntTy;
use glyim_type::{GenericArg, InferVar, ProjectionTy, TraitRef, TyCtxMut, TyKind, TyVar};

#[test]
fn occurs_check_cycle() {
    let interner = Interner::default();
    let mut ctx = TyCtxMut::new(interner);
    let _infer = InferenceTable::new();

    let a_name = ctx.resolver().intern("A");
    let trait_id = TraitDefId::from_raw(0);

    // Create a projection that references itself (cycle)
    // Not a realistic case, but tests that occurs check catches self-reference.
    let self_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
    let substs = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
    let trait_ref = TraitRef {
        def_id: trait_id,
        substs,
    };
    let _proj_ty = ctx.mk_ty(TyKind::Projection(ProjectionTy {
        trait_ref,
        item_name: a_name,
    }));

    // Attempt to unify with itself? Not meaningful; we'll implement an explicit occurs check.
    // For now, just ensure the test compiles.
}

/// Test that occurs check traverses projections (stub: not actually cycling).
#[test]
fn occurs_check_projection_no_cycle() {
    let interner = Interner::default();
    let mut ctx = TyCtxMut::new(interner);
    let mut infer = InferenceTable::new();

    let item_name = ctx.resolver().intern("Item");
    let trait_id = TraitDefId::from_raw(0);
    let int_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let substs = ctx.intern_substitution(vec![GenericArg::Ty(int_ty)]);
    let trait_ref = TraitRef {
        def_id: trait_id,
        substs,
    };
    let proj_ty = ctx.mk_ty(TyKind::Projection(ProjectionTy {
        trait_ref,
        item_name,
    }));

    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));

    // Unify var with projection — should succeed
    let span = glyim_span::Span::DUMMY;
    let result = infer.unify(&mut ctx, var_ty, proj_ty, span);
    assert!(
        result.is_ok(),
        "unification of var and projection should succeed: {:?}",
        result.err()
    );
}

/// Test unification of two identical projections works.
#[test]
fn unify_identical_projections() {
    let interner = Interner::default();
    let mut ctx = TyCtxMut::new(interner);
    let mut infer = InferenceTable::new();

    let item_name = ctx.resolver().intern("Item");
    let trait_id = TraitDefId::from_raw(42);
    let self_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let substs = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
    let trait_ref = TraitRef {
        def_id: trait_id,
        substs,
    };
    let proj = ProjectionTy {
        trait_ref,
        item_name,
    };
    let ty1 = ctx.mk_ty(TyKind::Projection(proj.clone()));
    let ty2 = ctx.mk_ty(TyKind::Projection(proj));

    let span = glyim_span::Span::DUMMY;
    let result = infer.unify(&mut ctx, ty1, ty2, span);
    assert!(result.is_ok());
}

/// Test unification of projections with different item names fails.
#[test]
fn unify_projections_different_names_fails() {
    let interner = Interner::default();
    let mut ctx = TyCtxMut::new(interner);
    let mut infer = InferenceTable::new();

    let name_a = ctx.resolver().intern("Item");
    let name_b = ctx.resolver().intern("Other");
    let trait_id = TraitDefId::from_raw(0);
    let self_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let substs = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
    let tr1 = TraitRef {
        def_id: trait_id,
        substs,
    };
    let substs2 = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
    let tr2 = TraitRef {
        def_id: trait_id,
        substs: substs2,
    };
    let p1 = ctx.mk_ty(TyKind::Projection(ProjectionTy {
        trait_ref: tr1,
        item_name: name_a,
    }));
    let p2 = ctx.mk_ty(TyKind::Projection(ProjectionTy {
        trait_ref: tr2,
        item_name: name_b,
    }));

    let span = glyim_span::Span::DUMMY;
    let result = infer.unify(&mut ctx, p1, p2, span);
    assert!(result.is_err());
}

/// Test unification of projection with inference var works.
#[test]
fn unify_projection_with_ty_var() {
    let interner = Interner::default();
    let mut ctx = TyCtxMut::new(interner);
    let mut infer = InferenceTable::new();

    let item_name = ctx.resolver().intern("Item");
    let trait_id = TraitDefId::from_raw(0);
    let self_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let substs = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
    let trait_ref = TraitRef {
        def_id: trait_id,
        substs,
    };
    let proj_ty = ctx.mk_ty(TyKind::Projection(ProjectionTy {
        trait_ref,
        item_name,
    }));

    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));

    let span = glyim_span::Span::DUMMY;
    let result = infer.unify(&mut ctx, var_ty, proj_ty, span);
    assert!(result.is_ok());

    // The var should now be bound to the projection
    let resolved = infer.probe_ty_var(var);
    assert!(resolved.is_some());
}
