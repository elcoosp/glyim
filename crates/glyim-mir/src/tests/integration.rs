use crate::*;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{BinOp, Mutability, UnOp};
use glyim_span::Span;
use glyim_type::Ty;

fn make_source_info() -> SourceInfo {
    SourceInfo::new(Span::DUMMY)
}

#[test]
fn build_simple_function_body() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));

    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: make_source_info(),
    });

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    }));

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    assert_eq!(body.locals.len(), 1);
    assert_eq!(body.basic_blocks.len(), 1);
    assert_eq!(body.return_ty, Ty::UNIT);
    assert!(body.args().is_empty());
}

#[test]
fn build_function_with_args_and_locals() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1));

    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: make_source_info(),
    });
    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: make_source_info(),
    });
    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Mut,
        source_info: make_source_info(),
    });

    let mut basic_blocks = IndexVec::new();
    let _bb0 = basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
            switch_ty: Ty::BOOL,
            targets: SwitchTargets::if_switch(
                BasicBlockIdx::from_raw(1),
                BasicBlockIdx::from_raw(2),
            ),
        },
        source_info: make_source_info(),
    }));

    let _bb1 = basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_raw(2),
        },
        source_info: make_source_info(),
    }));

    let _bb2 = basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    }));

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 1,
        return_ty: Ty::BOOL,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    assert_eq!(body.locals.len(), 3);
    assert_eq!(body.arg_count, 1);
    assert_eq!(body.args().len(), 1);
    assert_eq!(body.args()[0].ty, Ty::BOOL);
    assert_eq!(body.basic_blocks.len(), 3);
}

#[test]
fn build_body_with_assignments() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(2));

    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: make_source_info(),
    });
    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Mut,
        source_info: make_source_info(),
    });

    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::StorageLive(LocalIdx::from_raw(1)),
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Bool(true),
                ty: Ty::BOOL,
                span: Span::DUMMY,
            })),
        ),
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::StorageDead(LocalIdx::from_raw(1)),
        source_info: make_source_info(),
    });

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(bb0);

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    assert_eq!(
        body.basic_blocks[BasicBlockIdx::from_raw(0)]
            .statements
            .len(),
        3
    );
    assert!(matches!(
        body.basic_blocks[BasicBlockIdx::from_raw(0)].statements[0].kind,
        StatementKind::StorageLive(_)
    ));
    assert!(matches!(
        body.basic_blocks[BasicBlockIdx::from_raw(0)].statements[1].kind,
        StatementKind::Assign(_, _)
    ));
    assert!(matches!(
        body.basic_blocks[BasicBlockIdx::from_raw(0)].statements[2].kind,
        StatementKind::StorageDead(_)
    ));
}

#[test]
fn build_body_with_call() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(3));

    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: make_source_info(),
    });
    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: make_source_info(),
    });

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Uint(0),
                ty: Ty::BOOL,
                span: Span::DUMMY,
            }),
            args: vec![Operand::Copy(Place::new(LocalIdx::from_raw(1)))],
            destination: Place::new(LocalIdx::from_raw(1)),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: Some(BasicBlockIdx::from_raw(2)),
        },
        source_info: make_source_info(),
    }));
    basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    }));
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: make_source_info(),
        },
        is_cleanup: true,
    });

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    assert_eq!(body.basic_blocks.len(), 3);
    assert!(body.basic_blocks[BasicBlockIdx::from_raw(2)].is_cleanup);
    assert!(matches!(
        body.basic_blocks[BasicBlockIdx::from_raw(0)]
            .terminator
            .kind,
        TerminatorKind::Call { .. }
    ));
}

#[test]
fn build_body_with_assert() {
    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Assert {
            cond: Operand::Constant(MirConst {
                kind: MirConstKind::Bool(true),
                ty: Ty::BOOL,
                span: Span::DUMMY,
            }),
            expected: true,
            target: BasicBlockIdx::from_raw(1),
            cleanup: None,
            msg: AssertMessage::BoundsCheck,
        },
        source_info: make_source_info(),
    }));
    basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    }));

    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals: IndexVec::from_raw(vec![LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: make_source_info(),
        }]),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    assert_eq!(body.basic_blocks.len(), 2);
    if let TerminatorKind::Assert { msg, .. } = &body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .terminator
        .kind
    {
        assert!(matches!(msg, AssertMessage::BoundsCheck));
    } else {
        panic!("Expected Assert terminator");
    }
}

#[test]
fn build_body_with_binary_op_rvalue() {
    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::BinaryOp(
                BinOp::Add,
                [
                    Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                    Operand::Copy(Place::new(LocalIdx::from_raw(3))),
                ],
            ),
        ),
        source_info: make_source_info(),
    });

    if let StatementKind::Assign(_, Rvalue::BinaryOp(op, _)) = &bb0.statements[0].kind {
        assert_eq!(*op, BinOp::Add);
    } else {
        panic!("Expected Assign with BinaryOp");
    }
}

#[test]
fn build_body_with_unary_op_rvalue() {
    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::UnaryOp(UnOp::Not, Operand::Copy(Place::new(LocalIdx::from_raw(2)))),
        ),
        source_info: make_source_info(),
    });

    if let StatementKind::Assign(_, Rvalue::UnaryOp(op, _)) = &bb0.statements[0].kind {
        assert_eq!(*op, UnOp::Not);
    } else {
        panic!("Expected Assign with UnaryOp");
    }
}

#[test]
fn build_body_with_ref_rvalue() {
    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Ref(
                Place::new(LocalIdx::from_raw(2)),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
        ),
        source_info: make_source_info(),
    });

    if let StatementKind::Assign(
        _,
        Rvalue::Ref(
            _,
            BorrowKind::Mut {
                allow_two_phase_borrow,
            },
        ),
    ) = &bb0.statements[0].kind
    {
        assert!(!allow_two_phase_borrow);
    } else {
        panic!("Expected Assign with Ref");
    }
}

#[test]
fn build_body_with_discriminant_and_len() {
    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Discriminant(Place::new(LocalIdx::from_raw(2))),
        ),
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(3)),
            Rvalue::Len(Place::new(LocalIdx::from_raw(4))),
        ),
        source_info: make_source_info(),
    });

    assert_eq!(bb0.statements.len(), 2);
    assert!(matches!(
        bb0.statements[0].kind,
        StatementKind::Assign(_, Rvalue::Discriminant(_))
    ));
    assert!(matches!(
        bb0.statements[1].kind,
        StatementKind::Assign(_, Rvalue::Len(_))
    ));
}

#[test]
fn build_body_with_cast_and_repeat() {
    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Cast(
                CastKind::IntToInt,
                Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                Ty::BOOL,
            ),
        ),
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(3)),
            Rvalue::Repeat(
                Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(false),
                    ty: Ty::BOOL,
                    span: Span::DUMMY,
                }),
                MirConst {
                    kind: MirConstKind::Uint(5),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                },
            ),
        ),
        source_info: make_source_info(),
    });

    assert_eq!(bb0.statements.len(), 2);
    assert!(matches!(
        bb0.statements[0].kind,
        StatementKind::Assign(_, Rvalue::Cast(CastKind::IntToInt, _, _))
    ));
    assert!(matches!(
        bb0.statements[1].kind,
        StatementKind::Assign(_, Rvalue::Repeat(_, _))
    ));
}

#[test]
fn build_body_with_aggregate_tuple() {
    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: make_source_info(),
    });

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Aggregate(
                AggregateKind::Tuple,
                vec![
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: Ty::BOOL,
                        span: Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(2),
                        ty: Ty::BOOL,
                        span: Span::DUMMY,
                    }),
                ],
            ),
        ),
        source_info: make_source_info(),
    });

    if let StatementKind::Assign(_, Rvalue::Aggregate(AggregateKind::Tuple, ops)) =
        &bb0.statements[0].kind
    {
        assert_eq!(ops.len(), 2);
    } else {
        panic!("Expected Assign with Aggregate Tuple");
    }
}
