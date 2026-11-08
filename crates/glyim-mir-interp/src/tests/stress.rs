use super::helpers::*;
use crate::{InterpValue, Interpreter};
use glyim_core::*;
use glyim_mir::*;
use glyim_type::{FieldIdx, Ty, TyKind};

#[test]
fn test_many_statements_in_block() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_sum = add_local(&mut body, i32_ty, Mutability::Mut);

    let bb0 = BasicBlockIdx::from_raw(0);
    // sum = 0; sum += 1; sum += 2; ... sum += 9; (10 operations)
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_sum), Rvalue::Use(const_int(0))),
    );
    for i in 1..=9 {
        add_statement(
            &mut body,
            bb0,
            StatementKind::Assign(
                Place::new(local_sum),
                Rvalue::BinaryOp(
                    BinOp::Add,
                    Box::new((Operand::Copy(Place::new(local_sum)), const_int(i))),
                ),
            ),
        );
    }

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
    // sum of 1..9 = 45
    assert_eq!(
        *interp.get_local_value(local_sum).unwrap(),
        InterpValue::Int(45)
    );
}

#[test]
fn test_nested_aggregate_5_levels() {
    // Build: (((((42,),),),),) and read deepest value
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);

    // We'll build 5 nested tuples, each with one element
    let locals: Vec<_> = (0..6)
        .map(|_| add_local(&mut body, i32_ty, Mutability::Mut))
        .collect();
    let result_local = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_raw(0);

    // locals[0] = (42,)
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(locals[0]),
            Rvalue::Aggregate(AggregateKind::Tuple, vec![const_int(42)]),
        ),
    );
    // Wrap each level
    for i in 1..5 {
        add_statement(
            &mut body,
            bb0,
            StatementKind::Assign(
                Place::new(locals[i]),
                Rvalue::Aggregate(
                    AggregateKind::Tuple,
                    vec![Operand::Copy(Place::new(locals[i - 1]))],
                ),
            ),
        );
    }

    // Read through 5 field-0 projections
    let proj: Vec<ProjectionElem> = (0..5)
        .map(|_| ProjectionElem::Field(FieldIdx::from_raw(0)))
        .collect();
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Copy(Place {
                local: locals[4],
                projection: proj.into_boxed_slice(),
            })),
        ),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
    assert_eq!(
        *interp.get_local_value(result_local).unwrap(),
        InterpValue::Int(42)
    );
}

#[test]
fn test_all_binary_ops_on_ints() {
    let ops = [
        (BinOp::Add, (10, 3), 13),
        (BinOp::Sub, (10, 3), 7),
        (BinOp::Mul, (10, 3), 30),
        (BinOp::Div, (10, 3), 3),
        (BinOp::Rem, (10, 3), 1),
        (BinOp::BitAnd, (6, 3), 2),
        (BinOp::BitOr, (6, 3), 7),
        (BinOp::BitXor, (6, 3), 5),
        (BinOp::Shl, (1, 4), 16),
        (BinOp::Shr, (16, 2), 4),
    ];

    for (op, (a, b), expected) in &ops {
        let mut ctx = glyim_test::test_ty_ctx();
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

        let mut body = empty_body(Ty::UNIT);
        let local_r = add_local(&mut body, i32_ty, Mutability::Mut);
        let bb0 = BasicBlockIdx::from_raw(0);
        add_statement(
            &mut body,
            bb0,
            StatementKind::Assign(
                Place::new(local_r),
                Rvalue::BinaryOp(*op, Box::new((const_int(*a), const_int(*b)))),
            ),
        );
        let tcx = ctx.freeze();
        let mut interp = Interpreter::new(&tcx);
        interp.add_function(
            DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            body,
        );
        interp
            .run_body(&interp.function_table.values().next().unwrap().clone())
            .unwrap();
        assert_eq!(
            *interp.get_local_value(local_r).unwrap(),
            InterpValue::Int(*expected),
            "failed for op {op:?}"
        );
    }
}

#[test]
fn test_comparison_ops() {
    let tests = [
        (BinOp::Eq, (5, 5), true),
        (BinOp::Eq, (5, 3), false),
        (BinOp::Ne, (5, 3), true),
        (BinOp::Lt, (3, 5), true),
        (BinOp::Lt, (5, 3), false),
        (BinOp::Gt, (5, 3), true),
        (BinOp::LtEq, (5, 5), true),
        (BinOp::LtEq, (6, 5), false),
        (BinOp::GtEq, (5, 5), true),
    ];

    for (op, (a, b), expected) in &tests {
        let ctx = glyim_test::test_ty_ctx();

        let mut body = empty_body(Ty::UNIT);
        let local_r = add_local(&mut body, Ty::BOOL, Mutability::Mut);
        let bb0 = BasicBlockIdx::from_raw(0);
        add_statement(
            &mut body,
            bb0,
            StatementKind::Assign(
                Place::new(local_r),
                Rvalue::BinaryOp(*op, Box::new((const_int(*a), const_int(*b)))),
            ),
        );
        let tcx = ctx.freeze();
        let mut interp = Interpreter::new(&tcx);
        interp.add_function(
            DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            body,
        );
        interp
            .run_body(&interp.function_table.values().next().unwrap().clone())
            .unwrap();
        assert_eq!(
            *interp.get_local_value(local_r).unwrap(),
            InterpValue::Bool(*expected),
            "failed for op {op:?}"
        );
    }
}

#[test]
fn test_division_by_zero_panics() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_r = add_local(&mut body, i32_ty, Mutability::Mut);
    let bb0 = BasicBlockIdx::from_raw(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_r),
            Rvalue::BinaryOp(BinOp::Div, Box::new((const_int(42), const_int(0)))),
        ),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_err());
}

#[test]
fn test_unary_not_and_neg() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    // Test Neg
    let mut body = empty_body(Ty::UNIT);
    let local_r = add_local(&mut body, i32_ty, Mutability::Mut);
    let bb0 = BasicBlockIdx::from_raw(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_r),
            Rvalue::UnaryOp(UnOp::Neg, const_int(5)),
        ),
    );
    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    interp
        .run_body(&interp.function_table.values().next().unwrap().clone())
        .unwrap();
    assert_eq!(
        *interp.get_local_value(local_r).unwrap(),
        InterpValue::Int(-5)
    );

    // Test Not
    let ctx2 = glyim_test::test_ty_ctx();
    let mut body = empty_body(Ty::UNIT);
    let local_r = add_local(&mut body, Ty::BOOL, Mutability::Mut);
    let bb0_not = BasicBlockIdx::from_raw(0);
    add_statement(
        &mut body,
        bb0_not,
        StatementKind::Assign(
            Place::new(local_r),
            Rvalue::UnaryOp(UnOp::Not, const_bool(true)),
        ),
    );
    let tcx2 = ctx2.freeze();
    let mut interp2 = Interpreter::new(&tcx2);
    interp2.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    interp2
        .run_body(&interp2.function_table.values().next().unwrap().clone())
        .unwrap();
    assert_eq!(
        *interp2.get_local_value(local_r).unwrap(),
        InterpValue::Bool(false)
    );
}

#[test]
fn test_storage_live_dead() {
    // StorageLive/StorageDead are no-ops; verify they don't crash and values persist across them
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_x = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_y = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_raw(0);
    add_statement(&mut body, bb0, StatementKind::StorageLive(local_x));
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(88))),
    );
    add_statement(&mut body, bb0, StatementKind::StorageDead(local_x));
    // Read after StorageDead should still work (interpreter is lenient)
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_y),
            Rvalue::Use(Operand::Copy(Place::new(local_x))),
        ),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    interp
        .run_body(&interp.function_table.values().next().unwrap().clone())
        .unwrap();
    assert_eq!(
        *interp.get_local_value(local_y).unwrap(),
        InterpValue::Int(88)
    );
}

#[test]
fn test_nop_statement_ignored() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_x = add_local(&mut body, i32_ty, Mutability::Mut);

    let bb0 = BasicBlockIdx::from_raw(0);
    add_statement(&mut body, bb0, StatementKind::Nop);
    add_statement(&mut body, bb0, StatementKind::Nop);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(1))),
    );
    add_statement(&mut body, bb0, StatementKind::Nop);

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    interp
        .run_body(&interp.function_table.values().next().unwrap().clone())
        .unwrap();
    assert_eq!(
        *interp.get_local_value(local_x).unwrap(),
        InterpValue::Int(1)
    );
}

#[test]
fn test_len_on_different_array_sizes() {
    for size in &[0u64, 1, 5, 100] {
        let mut ctx = glyim_test::test_ty_ctx();
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let array_ty = mk_array_ty(&mut ctx, i32_ty, *size);

        let mut body = empty_body(Ty::UNIT);
        let local_arr = add_local(&mut body, array_ty, Mutability::Not);
        let local_len = add_local(
            &mut body,
            ctx.mk_ty(TyKind::Int(IntTy::I64)),
            Mutability::Mut,
        );

        let bb0 = BasicBlockIdx::from_raw(0);
        add_statement(
            &mut body,
            bb0,
            StatementKind::Assign(Place::new(local_len), Rvalue::Len(Place::new(local_arr))),
        );

        let tcx = ctx.freeze();
        let mut interp = Interpreter::new(&tcx);
        interp.add_function(
            DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            body,
        );
        interp
            .run_body(&interp.function_table.values().next().unwrap().clone())
            .unwrap();
        assert_eq!(
            *interp.get_local_value(local_len).unwrap(),
            InterpValue::Int(i128::from(*size)),
            "failed for size {size}"
        );
    }
}

#[test]
fn test_get_local_value_uninitialized() {
    let ctx = glyim_test::test_frozen_ty_ctx();
    let interp = Interpreter::new(&ctx);
    assert!(interp.get_local_value(LocalIdx::from_raw(0)).is_none());
}
