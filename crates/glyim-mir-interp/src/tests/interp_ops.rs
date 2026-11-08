use glyim_core::{BinOp, CrateId, DefId, IntTy, LocalDefId, Mutability, UintTy, UnOp};
use glyim_mir::{
    AggregateKind, BasicBlockData, BasicBlockIdx, Body, BorrowKind, LocalDecl, MirConst,
    MirConstKind, Operand, Place, ProjectionElem, Rvalue, SourceInfo, Statement, StatementKind,
    SwitchTargets, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Const, ConstKind, FieldIdx, Ty, TyCtxMut, TyKind};

use crate::{InterpValue, Interpreter};

fn dummy_owner() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

fn dummy_source_info() -> SourceInfo {
    SourceInfo::new(Span::DUMMY)
}

fn make_i32_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Int(IntTy::I32))
}

fn make_usize_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Uint(UintTy::Usize))
}

fn make_array_ty(ctx: &mut TyCtxMut, elem_ty: Ty, len: u64) -> Ty {
    let len_const = Const {
        kind: ConstKind::Uint(u128::from(len)),
        ty: make_usize_ty(ctx),
    };
    ctx.mk_ty(TyKind::Array(elem_ty, len_const))
}

#[test]
fn len_on_array_returns_correct_integer() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = make_i32_ty(&mut ctx_mut);
    let array_ty = make_array_ty(&mut ctx_mut, i32_ty, 5);
    let usize_ty = make_usize_ty(&mut ctx_mut);
    let tcx = ctx_mut.freeze();

    let mut body = Body::dummy(dummy_owner());
    let bb0 = BasicBlockIdx::from_raw(0);
    let array_local = body.locals.push(LocalDecl {
        ty: array_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });
    let result_local = body.locals.push(LocalDecl {
        ty: usize_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    body.basic_blocks[bb0].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_source_info(),
    };
    body.basic_blocks[bb0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(result_local),
            Rvalue::Len(Place::new(array_local)),
        ),
        source_info: dummy_source_info(),
    });

    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();

    let result = interp.get_local_value(result_local).unwrap();
    assert_eq!(result, &InterpValue::Int(5));
}

#[test]
fn match_on_enum_selects_correct_arm_via_discriminant() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = make_i32_ty(&mut ctx_mut);
    let tcx = ctx_mut.freeze();

    let mut body = Body::dummy(dummy_owner());
    let bb0 = BasicBlockIdx::from_raw(0);

    let enum_local = body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    let result_local = body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    body.basic_blocks[bb0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(enum_local),
            Rvalue::Aggregate(
                AggregateKind::Tuple,
                vec![
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(42),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                ],
            ),
        ),
        source_info: dummy_source_info(),
    });

    let bb1 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_source_info(),
    }));
    body.basic_blocks[bb1].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(100),
                ty: i32_ty,
                span: Span::DUMMY,
            })),
        ),
        source_info: dummy_source_info(),
    });

    let bb2 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_source_info(),
    }));
    body.basic_blocks[bb2].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(200),
                ty: i32_ty,
                span: Span::DUMMY,
            })),
        ),
        source_info: dummy_source_info(),
    });

    let bb3 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_source_info(),
    }));
    body.basic_blocks[bb3].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(999),
                ty: i32_ty,
                span: Span::DUMMY,
            })),
        ),
        source_info: dummy_source_info(),
    });

    let discr_local = body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });
    body.basic_blocks[bb0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(discr_local),
            Rvalue::Discriminant(Place::new(enum_local)),
        ),
        source_info: dummy_source_info(),
    });

    body.basic_blocks[bb0].terminator = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(discr_local)),
            switch_ty: i32_ty,
            targets: SwitchTargets::new(Box::new([(0, bb1), (1, bb2)]), bb3),
        },
        source_info: dummy_source_info(),
    };

    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();

    let result = interp.get_local_value(result_local).unwrap();
    assert_eq!(result, &InterpValue::Int(200));
}

#[test]
fn bitwise_and_on_integers() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = make_i32_ty(&mut ctx_mut);
    let tcx = ctx_mut.freeze();

    let mut body = Body::dummy(dummy_owner());
    let bb0 = BasicBlockIdx::from_raw(0);
    let result_local = body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    body.basic_blocks[bb0].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_source_info(),
    };
    body.basic_blocks[bb0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(result_local),
            Rvalue::BinaryOp(
                BinOp::BitAnd,
                Box::new((
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(0b1100),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(0b1010),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                )),
            ),
        ),
        source_info: dummy_source_info(),
    });

    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();

    let result = interp.get_local_value(result_local).unwrap();
    assert_eq!(result, &InterpValue::Int(0b1000));
}

#[test]
fn write_through_deref_field_modifies_nested_value() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = make_i32_ty(&mut ctx_mut);
    let tcx = ctx_mut.freeze();

    let mut body = Body::dummy(dummy_owner());
    let bb0 = BasicBlockIdx::from_raw(0);

    let aggregate_local = body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Mut,
        source_info: dummy_source_info(),
    });

    let ref_local = body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Mut,
        source_info: dummy_source_info(),
    });

    body.basic_blocks[bb0].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_source_info(),
    };

    body.basic_blocks[bb0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(aggregate_local),
            Rvalue::Aggregate(
                AggregateKind::Tuple,
                vec![
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(10),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(20),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                ],
            ),
        ),
        source_info: dummy_source_info(),
    });

    body.basic_blocks[bb0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(ref_local),
            Rvalue::Ref(Place::new(aggregate_local), BorrowKind::Shared),
        ),
        source_info: dummy_source_info(),
    });

    let target_place = Place {
        local: ref_local,
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_raw(0)),
        ]),
    };
    body.basic_blocks[bb0].statements.push(Statement {
        kind: StatementKind::Assign(
            target_place,
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(99),
                ty: i32_ty,
                span: Span::DUMMY,
            })),
        ),
        source_info: dummy_source_info(),
    });

    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();

    let aggregate = interp.get_local_value(aggregate_local).unwrap();
    assert_eq!(
        aggregate,
        &InterpValue::Aggregate(vec![InterpValue::Int(99), InterpValue::Int(20)])
    );
}

#[test]
fn not_on_bool() {
    let ctx_mut = test_ty_ctx();
    let bool_ty = ctx_mut.bool_ty();
    let tcx = ctx_mut.freeze();

    let mut body = Body::dummy(dummy_owner());
    let bb0 = BasicBlockIdx::from_raw(0);
    let result_local = body.locals.push(LocalDecl {
        ty: bool_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    body.basic_blocks[bb0].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_source_info(),
    };
    body.basic_blocks[bb0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(result_local),
            Rvalue::UnaryOp(
                UnOp::Not,
                Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(true),
                    ty: bool_ty,
                    span: Span::DUMMY,
                }),
            ),
        ),
        source_info: dummy_source_info(),
    });

    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();

    let result = interp.get_local_value(result_local).unwrap();
    assert_eq!(result, &InterpValue::Bool(false));
}

#[test]
fn neg_on_int() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = make_i32_ty(&mut ctx_mut);
    let tcx = ctx_mut.freeze();

    let mut body = Body::dummy(dummy_owner());
    let bb0 = BasicBlockIdx::from_raw(0);
    let result_local = body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    body.basic_blocks[bb0].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_source_info(),
    };
    body.basic_blocks[bb0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(result_local),
            Rvalue::UnaryOp(
                UnOp::Neg,
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(42),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
            ),
        ),
        source_info: dummy_source_info(),
    });

    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();

    let result = interp.get_local_value(result_local).unwrap();
    assert_eq!(result, &InterpValue::Int(-42));
}
