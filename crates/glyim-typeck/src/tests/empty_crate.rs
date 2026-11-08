use super::test_utils::empty_def_map;
use crate::typeck_crate;
use glyim_hir::CrateHir;
use glyim_test::{assert_no_errors, mock::MockSolver};

#[test]
fn empty_crate_no_errors() {
    let ctx = super::test_utils::make_ty_ctx();
    let def_map = empty_def_map();
    let hir = CrateHir {
        items: glyim_core::arena::IndexVec::default(),
        bodies: glyim_core::arena::IndexVec::default(),
        body_owners: glyim_core::arena::IndexVec::default(),
        interner: glyim_core::Interner::default(),
    };
    let mut solver = MockSolver::new().respond_for_any(glyim_solve::SolverResult::Proven);
    let (_, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert_no_errors(&result.diagnostics);
}
