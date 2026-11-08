use crate::{InterpValue, Interpreter};
use glyim_core::{CrateId, DefId, IndexVec, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_type::{FieldIdx, Ty};

/// S19-T04: Write through Deref+Field projection updates nested value
///
/// Scenario:
/// - local 0: return place
/// - local 1: a reference (Ref) pointing to local 2
/// - local 2: an Aggregate with two fields
/// - local 3: index for the field we want to write
///
/// We assign to (*local `1).field_0` and check that local 2's first field is updated.
#[test]
fn write_through_deref_field_updates_nested_value() {
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);

    let crate_id = CrateId::from_raw(0);
    let local_def_id = LocalDefId::from_raw(0);
    let owner = DefId::new(crate_id, local_def_id);

    // 4 locals: return, ref, struct, unused
    let mut locals = IndexVec::with_capacity(4);
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    // Statement 1: Assign local 2 = Aggregate([Int(100), Int(200)])
    let agg_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(2)),
            Rvalue::Aggregate(
                AggregateKind::Tuple,
                vec![
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(100),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(200),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                ],
            ),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    // Statement 2: Assign local 1 = Ref(local 2) — we use Rvalue::Ref to create the ref
    let ref_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Ref(
                Place::new(LocalIdx::from_raw(2)),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    // Statement 3: Assign (*local 1).0 = Int(999)
    // Place: local=1, projection=[Deref, Field(0)]
    let deref_field_place = Place {
        local: LocalIdx::from_raw(1),
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_raw(0)),
        ]),
    };
    let write_stmt = Statement {
        kind: StatementKind::Assign(
            deref_field_place,
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(999),
                ty: Ty::UNIT,
                span: glyim_span::Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    // Statement 4: Assign local 0 = (*local 1).0  (read back the field)
    let read_place = Place {
        local: LocalIdx::from_raw(1),
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_raw(0)),
        ]),
    };
    let read_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Use(Operand::Copy(read_place)),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![agg_stmt, ref_stmt, write_stmt, read_stmt],
        terminator,
        is_cleanup: false,
    });

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    let result = interp.run_body(&body);
    assert!(result.is_ok(), "run_body failed: {result:?}");

    let ret = interp.get_local_value(LocalIdx::from_raw(0));
    assert!(ret.is_some(), "return place not set");
    assert_eq!(
        ret.unwrap(),
        &InterpValue::Int(999),
        "Deref+Field write should update nested field to 999"
    );

    // Also verify the struct in local 2 has been updated
    let struct_val = interp.get_local_value(LocalIdx::from_raw(2));
    assert!(struct_val.is_some(), "struct local not set");
    match struct_val.unwrap() {
        InterpValue::Aggregate(fields) => {
            assert_eq!(fields[0], InterpValue::Int(999), "field 0 should be 999");
            assert_eq!(
                fields[1],
                InterpValue::Int(200),
                "field 1 should be unchanged at 200"
            );
        }
        other => panic!("expected Aggregate, got {other:?}"),
    }
}

/// S19-T04b: Write through Deref only (no further projections)
#[test]
fn write_through_deref_only() {
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);

    let crate_id = CrateId::from_raw(0);
    let local_def_id = LocalDefId::from_raw(0);
    let owner = DefId::new(crate_id, local_def_id);

    let mut locals = IndexVec::with_capacity(3);
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    // local 2 = Int(10)
    let init_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(2)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(10),
                ty: Ty::UNIT,
                span: glyim_span::Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    // local 1 = Ref(local 2)
    let ref_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Ref(
                Place::new(LocalIdx::from_raw(2)),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    // *local 1 = Int(99)
    let deref_place = Place {
        local: LocalIdx::from_raw(1),
        projection: Box::new([ProjectionElem::Deref]),
    };
    let write_stmt = Statement {
        kind: StatementKind::Assign(
            deref_place,
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(99),
                ty: Ty::UNIT,
                span: glyim_span::Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    // local 0 = *local 1
    let read_deref = Place {
        local: LocalIdx::from_raw(1),
        projection: Box::new([ProjectionElem::Deref]),
    };
    let read_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Use(Operand::Copy(read_deref)),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![init_stmt, ref_stmt, write_stmt, read_stmt],
        terminator,
        is_cleanup: false,
    });

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    let result = interp.run_body(&body);
    assert!(result.is_ok(), "run_body failed: {result:?}");

    let ret = interp.get_local_value(LocalIdx::from_raw(0));
    assert!(ret.is_some(), "return place not set");
    assert_eq!(
        ret.unwrap(),
        &InterpValue::Int(99),
        "Deref write should update to 99"
    );
}

/// S19-T04c: Write through Field projection only
#[test]
fn write_through_field_only() {
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);

    let crate_id = CrateId::from_raw(0);
    let local_def_id = LocalDefId::from_raw(0);
    let owner = DefId::new(crate_id, local_def_id);

    let mut locals = IndexVec::with_capacity(2);
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    // local 1 = Aggregate([Int(1), Int(2)])
    let agg_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Aggregate(
                AggregateKind::Tuple,
                vec![
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(2),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                ],
            ),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    // local 1.1 = Int(77)
    let field_place = Place {
        local: LocalIdx::from_raw(1),
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(1))]),
    };
    let write_stmt = Statement {
        kind: StatementKind::Assign(
            field_place,
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(77),
                ty: Ty::UNIT,
                span: glyim_span::Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    // local 0 = local 1.1
    let read_field = Place {
        local: LocalIdx::from_raw(1),
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(1))]),
    };
    let read_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Use(Operand::Copy(read_field)),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![agg_stmt, write_stmt, read_stmt],
        terminator,
        is_cleanup: false,
    });

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    let result = interp.run_body(&body);
    assert!(result.is_ok(), "run_body failed: {result:?}");

    let ret = interp.get_local_value(LocalIdx::from_raw(0));
    assert!(ret.is_some(), "return place not set");
    assert_eq!(
        ret.unwrap(),
        &InterpValue::Int(77),
        "Field write should update to 77"
    );
}
