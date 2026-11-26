use super::helpers::*;
use crate::Interpreter;
use glyim_core::*;
use glyim_mir::*;
use glyim_type::{Ty, TyKind};

#[test]
fn test_t05_cast_int_to_float() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));

    let mut body = empty_body(Ty::UNIT);
    let local_int = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_float = add_local(&mut body, f64_ty, Mutability::Mut);

    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_int), Rvalue::Use(const_int(42))),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_float),
            Rvalue::Cast(CastKind::IntToFloat, const_int(42), f64_ty),
        ),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    // Cast not implemented yet, expect error
    assert!(result.is_ok());
}
