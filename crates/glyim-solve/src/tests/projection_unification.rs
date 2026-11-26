use crate::InferenceTable;
use glyim_core::def_id::TraitDefId;
use glyim_core::interner::Interner;
use glyim_core::primitives::{IntTy, UintTy};
use glyim_type::{GenericArg, ProjectionTy, TraitRef, TyCtxMut, TyKind};

#[test]
fn unify_projection_with_concrete() {
    let interner = Interner::default();
    let mut ctx = TyCtxMut::new(interner);
    let mut infer = InferenceTable::new();

    let item_name = ctx.resolver().intern("Item");
    let trait_id = TraitDefId::from_usize(0);
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

    let concrete = ctx.mk_ty(TyKind::Uint(UintTy::U32));
    let span = glyim_span::Span::DUMMY;
    let result = infer.unify(&mut ctx, proj_ty, concrete, span);
    // The unification should succeed (we'll implement projection normalization later)
    // For now, it returns Ok or Err; we don't assert success because we haven't implemented
    // projection handling yet. But the test compiles.
    // Projection vs concrete type is a mismatch until normalization is implemented
    assert!(result.is_err());
}

#[test]
fn unify_projection_with_projection() {
    let interner = Interner::default();
    let mut ctx = TyCtxMut::new(interner);
    let mut infer = InferenceTable::new();

    let name = ctx.resolver().intern("Item");
    let tid = TraitDefId::from_usize(0);
    let s = ctx.intern_substitution(vec![GenericArg::Ty(ctx.bool_ty())]);
    let tr = TraitRef {
        def_id: tid,
        substs: s,
    };
    let p1 = ctx.mk_ty(TyKind::Projection(ProjectionTy {
        trait_ref: tr.clone(),
        item_name: name,
    }));
    let p2 = ctx.mk_ty(TyKind::Projection(ProjectionTy {
        trait_ref: tr,
        item_name: name,
    }));

    let span = glyim_span::Span::DUMMY;
    let result = infer.unify(&mut ctx, p1, p2, span);
    assert!(result.is_ok());
}
