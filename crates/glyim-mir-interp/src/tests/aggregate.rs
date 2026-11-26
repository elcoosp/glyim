use super::helpers::*;
use crate::Interpreter;
use glyim_core::*;
use glyim_mir::*;
use glyim_type::{FieldIdx, Ty, TyKind};

#[test]
fn test_t01_struct_construction_and_field_access() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = AdtId::from_usize(42);
    let subst = ctx.intern_substitution(vec![]);
    let struct_ty = ctx.mk_ty(TyKind::Adt(adt_id, subst));

    let mut body = empty_body(Ty::UNIT);
    let local_struct = add_local(&mut body, struct_ty, Mutability::Mut);
    let local_field0 = add_local(&mut body, i32_ty, Mutability::Not);
    let local_field1 = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(&mut body, bb0, StatementKind::StorageLive(local_struct));
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_struct),
            Rvalue::Aggregate(
                AggregateKind::Adt(adt_id, glyim_mir::VariantIdx::from_usize(0), subst),
                vec![const_int(42), const_int(10)],
            ),
        ),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_field0),
            Rvalue::Use(Operand::Copy(place_with_proj(
                local_struct,
                vec![ProjectionElem::Field(FieldIdx::from_usize(0))],
            ))),
        ),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_field1),
            Rvalue::Use(Operand::Copy(place_with_proj(
                local_struct,
                vec![ProjectionElem::Field(FieldIdx::from_usize(1))],
            ))),
        ),
    );
    add_statement(&mut body, bb0, StatementKind::StorageDead(local_struct));

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    // Currently projections not implemented, so an error is expected.
    assert!(result.is_ok());
}

#[test]
fn test_t02_tuple_construction_and_indexing() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let subst = ctx.intern_substitution(vec![
        glyim_type::GenericArg::Ty(i32_ty),
        glyim_type::GenericArg::Ty(i32_ty),
    ]);
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(subst));

    let mut body = empty_body(Ty::UNIT);
    let local_tuple = add_local(&mut body, tuple_ty, Mutability::Mut);
    let local_0 = add_local(&mut body, i32_ty, Mutability::Not);
    let local_1 = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(&mut body, bb0, StatementKind::StorageLive(local_tuple));
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_tuple),
            Rvalue::Aggregate(AggregateKind::Tuple, vec![const_int(1), const_int(2)]),
        ),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_0),
            Rvalue::Use(Operand::Copy(place_with_proj(
                local_tuple,
                vec![ProjectionElem::Field(FieldIdx::from_usize(0))],
            ))),
        ),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_1),
            Rvalue::Use(Operand::Copy(place_with_proj(
                local_tuple,
                vec![ProjectionElem::Field(FieldIdx::from_usize(1))],
            ))),
        ),
    );
    add_statement(&mut body, bb0, StatementKind::StorageDead(local_tuple));

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok()); // projections not yet implemented
}

#[test]
fn test_t03_array_literal_and_index() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let array_ty = mk_array_ty(&mut ctx, i32_ty, 3);

    let mut body = empty_body(Ty::UNIT);
    let local_arr = add_local(&mut body, array_ty, Mutability::Mut);
    let local_idx = add_local(
        &mut body,
        ctx.mk_ty(TyKind::Int(IntTy::I32)),
        Mutability::Mut,
    );
    let local_elem = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(&mut body, bb0, StatementKind::StorageLive(local_arr));
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_arr),
            Rvalue::Aggregate(
                AggregateKind::Array(i32_ty),
                vec![const_int(10), const_int(20), const_int(30)],
            ),
        ),
    );
    // store index 1 into local_idx
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_idx), Rvalue::Use(const_int(1))),
    );
    // read arr[local_idx]
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_elem),
            Rvalue::Use(Operand::Copy(place_with_proj(
                local_arr,
                vec![ProjectionElem::Index(local_idx)],
            ))),
        ),
    );
    add_statement(&mut body, bb0, StatementKind::StorageDead(local_arr));

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok()); // projections not yet implemented
}
