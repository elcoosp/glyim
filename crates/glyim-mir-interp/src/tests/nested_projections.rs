use super::helpers::*;
use crate::{InterpValue, Interpreter};
use glyim_core::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{FieldIdx, Ty, TyKind};

#[test]
fn test_nested_field_projection_read() {
    // struct Inner { x: i32, y: i32 }
    // struct Outer { a: Inner, b: i32 }
    // Read outer.a.y
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_outer = add_local(&mut body, i32_ty, Mutability::Mut);
    let _local_result = add_local(&mut body, i32_ty, Mutability::Not);

    // Build an aggregate: Outer = [Inner = [100, 200], 999]
    let inner_agg = InterpValue::Aggregate(vec![InterpValue::Int(100), InterpValue::Int(200)]);
    let _outer_agg = InterpValue::Aggregate(vec![inner_agg, InterpValue::Int(999)]);

    let bb0 = BasicBlockIdx::from_usize(0);
    // Store the outer aggregate
    body.basic_blocks[bb0].statements.insert(
        0,
        Statement {
            kind: StatementKind::Assign(
                Place::new(local_outer),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    );
    // We need to manually set the local value since we can't easily build complex constants.
    // Instead, we'll use a simpler approach: build the aggregate via statements.
    body.basic_blocks[bb0].statements.clear();

    // local_outer = Aggregate([Aggregate([100,200]), 999])
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_outer),
            Rvalue::Aggregate(
                AggregateKind::Tuple,
                vec![
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(100),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(200),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                ],
            ),
        ),
    );

    // We'll restructure: first build inner aggregate in local_inner, then outer
    body.locals = IndexVec::new();
    body.locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let local_inner = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_outer2 = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_result2 = add_local(&mut body, i32_ty, Mutability::Not);

    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_inner),
            Rvalue::Aggregate(AggregateKind::Tuple, vec![const_int(100), const_int(200)]),
        ),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_outer2),
            Rvalue::Aggregate(
                AggregateKind::Tuple,
                vec![Operand::Copy(Place::new(local_inner)), const_int(999)],
            ),
        ),
    );
    // Read outer2.0.1 (field 0, then field 1) -> should be 200
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_result2),
            Rvalue::Use(Operand::Copy(Place {
                local: local_outer2,
                projection: vec![
                    ProjectionElem::Field(FieldIdx::from_usize(0)),
                    ProjectionElem::Field(FieldIdx::from_usize(1)),
                ]
                .into_boxed_slice(),
            })),
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
    let val = interp.get_local_value(local_result2).unwrap();
    assert_eq!(*val, InterpValue::Int(200));
}

#[test]
fn test_write_through_field_projection() {
    // Build aggregate [10, 20], then write 99 to field 1, verify field 1 is 99
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_agg = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_field = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_agg),
            Rvalue::Aggregate(AggregateKind::Tuple, vec![const_int(10), const_int(20)]),
        ),
    );
    // Write 99 to field 1
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place {
                local: local_agg,
                projection: vec![ProjectionElem::Field(FieldIdx::from_usize(1))].into_boxed_slice(),
            },
            Rvalue::Use(const_int(99)),
        ),
    );
    // Read field 1 back
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_field),
            Rvalue::Use(Operand::Copy(Place {
                local: local_agg,
                projection: vec![ProjectionElem::Field(FieldIdx::from_usize(1))].into_boxed_slice(),
            })),
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
    let val = interp.get_local_value(local_field).unwrap();
    assert_eq!(*val, InterpValue::Int(99));
}

#[test]
fn test_deref_projection_read() {
    // let x: i32 = 42; let r: &i32 = &x; let y: i32 = *r;
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_x = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_r = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_y = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(42))),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_r),
            Rvalue::Ref(Place::new(local_x), BorrowKind::Shared),
        ),
    );
    // *r (Deref projection on local_r)
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_y),
            Rvalue::Use(Operand::Copy(Place {
                local: local_r,
                projection: vec![ProjectionElem::Deref].into_boxed_slice(),
            })),
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
    let val = interp.get_local_value(local_y).unwrap();
    assert_eq!(*val, InterpValue::Int(42));
}

#[test]
fn test_write_through_deref_projection() {
    // let x: i32 = 0; let r: &mut i32 = &x; *r = 77; assert x == 77;
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_x = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_r = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_check = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(0))),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_r),
            Rvalue::Ref(
                Place::new(local_x),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
        ),
    );
    // *r = 77
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place {
                local: local_r,
                projection: vec![ProjectionElem::Deref].into_boxed_slice(),
            },
            Rvalue::Use(const_int(77)),
        ),
    );
    // check x
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_check),
            Rvalue::Use(Operand::Copy(Place::new(local_x))),
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
    let val = interp.get_local_value(local_check).unwrap();
    assert_eq!(*val, InterpValue::Int(77));
}
