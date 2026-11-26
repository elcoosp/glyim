use super::helpers::*;
use crate::Interpreter;
use glyim_core::*;
use glyim_mir::*;
use glyim_type::{Region, Ty, TyKind};

#[test]
fn test_t07_ref_take_address_of_local() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Not);

    let mut body = empty_body(Ty::UNIT);
    let local_val = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_ref = add_local(&mut body, ref_ty, Mutability::Mut);

    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_val), Rvalue::Use(const_int(99))),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_ref),
            Rvalue::Ref(Place::new(local_val), BorrowKind::Shared),
        ),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
}
