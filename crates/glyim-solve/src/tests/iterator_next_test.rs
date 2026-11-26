use crate::*;
use glyim_core::def_id::FnDefId;
use glyim_test::test_ty_ctx;

#[test]
fn test_real_iterator_next_info_returns_some_when_builtin_set() {
    let mut ctx_mut = test_ty_ctx();
    let mut trait_ctx = TraitContext::new();

    // Set the builtin next function ID
    let next_def_id = FnDefId::from_usize(42);
    trait_ctx.set_builtin_iterator_next(next_def_id);

    let solver = SimpleTraitSolver::new(&trait_ctx);
    let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));

    let result = solver.iterator_next_info(&mut ctx_mut, i32_ty, i32_ty);
    assert!(result.is_some(), "Expected Some, got None");

    let info = result.unwrap();
    assert_eq!(info.fn_def_id, next_def_id);
    // Just verify the types are not error sentinel (use direct equality)
    assert_ne!(info.fn_ty, glyim_type::Ty::ERROR);
    assert_ne!(info.option_ty, glyim_type::Ty::ERROR);
    assert_ne!(info.ref_iter_ty, glyim_type::Ty::ERROR);
}

#[test]
fn test_real_iterator_next_info_returns_none_when_builtin_not_set() {
    let mut ctx_mut = test_ty_ctx();
    let trait_ctx = TraitContext::new();
    let solver = SimpleTraitSolver::new(&trait_ctx);
    let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));

    let result = solver.iterator_next_info(&mut ctx_mut, i32_ty, i32_ty);
    assert!(result.is_none(), "Expected None when builtin not set");
}
