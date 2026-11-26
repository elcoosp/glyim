use super::helpers::*;
use crate::{InterpValue, Interpreter};
use glyim_core::*;
use glyim_mir::*;
use glyim_type::{Ty, TyKind};

#[test]
fn test_t06_len_on_array() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let array_ty = mk_array_ty(&mut ctx, i32_ty, 5);

    let mut body = empty_body(Ty::UNIT);
    let local_arr = add_local(&mut body, array_ty, Mutability::Not);
    let local_len = add_local(
        &mut body,
        ctx.mk_ty(TyKind::Int(IntTy::I64)),
        Mutability::Mut,
    );

    let bb0 = BasicBlockIdx::from_usize(0);
    // local_arr doesn't need initialization; its type is known.
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_len), Rvalue::Len(Place::new(local_arr))),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
    let val = interp.get_local_value(local_len).unwrap();
    assert_eq!(*val, InterpValue::Int(5));
}
