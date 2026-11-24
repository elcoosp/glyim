use glyim_core::Mutability;
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{Ty, TyKind};

fn make_int_ty(ctx: &mut glyim_type::TyCtxMut) -> glyim_type::Ty {
    ctx.mk_ty(TyKind::Int(glyim_core::IntTy::I32))
}

fn make_const(val: i128, ty: glyim_type::Ty) -> MirConst {
    MirConst {
        kind: MirConstKind::Int(val),
        ty,
        span: glyim_span::Span::DUMMY,
    }
}

fn make_place(idx: u32) -> Place {
    Place::new(LocalIdx::from_raw(idx))
}

fn make_copy(idx: u32) -> Operand {
    Operand::Copy(make_place(idx))
}

fn make_body_two_const_chain(ctx: &mut glyim_type::TyCtxMut) -> Body {
    let int_ty = make_int_ty(ctx);
    let stmts = vec![
        Statement {
            kind: StatementKind::Assign(
                make_place(1),
                Rvalue::Use(Operand::Constant(make_const(10, int_ty))),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(make_place(2), Rvalue::Use(make_copy(1))),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                make_place(3),
                Rvalue::BinaryOp(
                    glyim_core::BinOp::Add,
                    [make_copy(2), Operand::Constant(make_const(5, int_ty))],
                ),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
    ];
    let block = BasicBlockData {
        statements: stmts,
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: false,
    };
    let mut body = Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(0),
    ));
    body.basic_blocks = glyim_core::IndexVec::from_raw(vec![block]);
    body.locals = glyim_core::IndexVec::from_raw(
        (0..4)
            .map(|_| LocalDecl {
                ty: int_ty,
                mutability: Mutability::Not,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            })
            .collect(),
    );
    body
}

fn make_body_overwrite(ctx: &mut glyim_type::TyCtxMut) -> Body {
    let int_ty = make_int_ty(ctx);
    let stmts = vec![
        Statement {
            kind: StatementKind::Assign(
                make_place(1),
                Rvalue::Use(Operand::Constant(make_const(10, int_ty))),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(make_place(1), Rvalue::Use(make_copy(0))),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                make_place(2),
                Rvalue::BinaryOp(
                    glyim_core::BinOp::Add,
                    [make_copy(1), Operand::Constant(make_const(1, int_ty))],
                ),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
    ];
    let block = BasicBlockData {
        statements: stmts,
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: false,
    };
    let mut body = Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(0),
    ));
    body.basic_blocks = glyim_core::IndexVec::from_raw(vec![block]);
    body.locals = glyim_core::IndexVec::from_raw(
        (0..3)
            .map(|_| LocalDecl {
                ty: int_ty,
                mutability: Mutability::Not,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            })
            .collect(),
    );
    body
}

fn make_body_projection_no_propagate(ctx: &mut glyim_type::TyCtxMut) -> Body {
    let int_ty = make_int_ty(ctx);
    let proj: Box<[ProjectionElem]> = vec![ProjectionElem::Deref].into();
    let stmts = vec![
        Statement {
            kind: StatementKind::Assign(
                make_place(1),
                Rvalue::Use(Operand::Constant(make_const(10, int_ty))),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                Place {
                    local: LocalIdx::from_raw(1),
                    projection: proj,
                },
                Rvalue::Use(Operand::Constant(make_const(99, int_ty))),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(make_place(2), Rvalue::Use(make_copy(1))),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
    ];
    let block = BasicBlockData {
        statements: stmts,
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: false,
    };
    let mut body = Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(0),
    ));
    body.basic_blocks = glyim_core::IndexVec::from_raw(vec![block]);
    body.locals = glyim_core::IndexVec::from_raw(
        (0..3)
            .map(|_| LocalDecl {
                ty: int_ty,
                mutability: Mutability::Not,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            })
            .collect(),
    );
    body
}

#[test]
fn propagate_int_constants() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let int_ty = make_int_ty(ctx_mut);
        let stmts = vec![
            Statement {
                kind: StatementKind::Assign(
                    make_place(1),
                    Rvalue::Use(Operand::Constant(make_const(42, int_ty))),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    make_place(2),
                    Rvalue::BinaryOp(
                        glyim_core::BinOp::Add,
                        [make_copy(1), Operand::Constant(make_const(10, int_ty))],
                    ),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
        ];
        let block = BasicBlockData {
            statements: stmts,
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        };
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(0),
            glyim_core::LocalDefId::from_raw(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_raw(vec![block]);
        body.locals = glyim_core::IndexVec::from_raw(
            (0..3)
                .map(|_| LocalDecl {
                    ty: int_ty,
                    mutability: Mutability::Not,
                    source_info: SourceInfo::new(glyim_span::Span::DUMMY),
                })
                .collect(),
        );
        body
    });
    crate::constant_prop::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_raw(0)];
    assert_eq!(
        block.statements.len(),
        2,
        "should still have two statements"
    );
    if let StatementKind::Assign(_, Rvalue::BinaryOp(_, box_ops)) = &block.statements[1].kind {
        assert!(
            matches!(box_ops[0], Operand::Constant(_)),
            "left operand should be a constant after propagation"
        );
    } else {
        panic!("second statement should be a BinaryOp");
    }
}

#[test]
fn propagate_through_chain() {
    let (ctx, mut body) = with_fresh_ty_ctx(make_body_two_const_chain);
    crate::constant_prop::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_raw(0)];
    if let StatementKind::Assign(_, Rvalue::BinaryOp(_, box_ops)) = &block.statements[2].kind {
        assert!(
            matches!(box_ops[0], Operand::Constant(_)),
            "left operand should be constant after chain propagation"
        );
        assert!(
            matches!(box_ops[1], Operand::Constant(_)),
            "right operand should be constant"
        );
    } else {
        panic!("third statement should be BinaryOp");
    }
}

#[test]
fn overwrite_removes_from_const_map() {
    let (ctx, mut body) = with_fresh_ty_ctx(make_body_overwrite);
    crate::constant_prop::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_raw(0)];
    if let StatementKind::Assign(_, Rvalue::BinaryOp(_, box_ops)) = &block.statements[2].kind {
        assert!(
            matches!(box_ops[0], Operand::Copy(_)),
            "left operand should NOT be constant after overwrite"
        );
    } else {
        panic!("third statement should be BinaryOp");
    }
}

#[test]
fn projection_write_prevents_propagation() {
    let (ctx, mut body) = with_fresh_ty_ctx(make_body_projection_no_propagate);
    crate::constant_prop::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_raw(0)];
    if let StatementKind::Assign(_, Rvalue::Use(op)) = &block.statements[2].kind {
        assert!(
            matches!(op, Operand::Copy(_)),
            "should remain Copy after projection write, not Constant"
        );
    } else {
        panic!("expected Use");
    }
}

#[test]
fn re_constant_after_overwrite_propagates() {
    // local1 = 10; local1 = local0; local1 = 20; local2 = local1 + 1
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let int_ty = make_int_ty(ctx_mut);
        let stmts = vec![
            Statement {
                kind: StatementKind::Assign(
                    make_place(1),
                    Rvalue::Use(Operand::Constant(make_const(10, int_ty))),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(make_place(1), Rvalue::Use(make_copy(0))),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    make_place(1),
                    Rvalue::Use(Operand::Constant(make_const(20, int_ty))),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    make_place(2),
                    Rvalue::BinaryOp(
                        glyim_core::BinOp::Add,
                        [make_copy(1), Operand::Constant(make_const(1, int_ty))],
                    ),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
        ];
        let block = BasicBlockData {
            statements: stmts,
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        };
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(0),
            glyim_core::LocalDefId::from_raw(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_raw(vec![block]);
        body.locals = glyim_core::IndexVec::from_raw(
            (0..3)
                .map(|_| LocalDecl {
                    ty: int_ty,
                    mutability: Mutability::Not,
                    source_info: SourceInfo::new(glyim_span::Span::DUMMY),
                })
                .collect(),
        );
        body
    });
    crate::constant_prop::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_raw(0)];
    if let StatementKind::Assign(_, Rvalue::BinaryOp(_, box_ops)) = &block.statements[3].kind {
        assert!(
            matches!(box_ops[0], Operand::Constant(_)),
            "left operand should be constant 20 after re-constant"
        );
    } else {
        panic!("expected BinaryOp");
    }
}

#[test]
fn cross_block_constant_propagation() {
    // block0: local1 = 5; goto block1
    // block1: local2 = local1 + 1; SwitchInt(local2) -> block2
    // block2: return
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let int_ty = make_int_ty(ctx_mut);
        let block0 = BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    make_place(1),
                    Rvalue::Use(Operand::Constant(make_const(5, int_ty))),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(1),
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    make_place(2),
                    Rvalue::BinaryOp(
                        glyim_core::BinOp::Add,
                        [make_copy(1), Operand::Constant(make_const(1, int_ty))],
                    ),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: make_copy(2),
                    switch_ty: int_ty,
                    targets: SwitchTargets::new(
                        vec![(0, BasicBlockIdx::from_raw(2))].into_boxed_slice(),
                        BasicBlockIdx::from_raw(2),
                    ),
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        };
        let block2 = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        };
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(0),
            glyim_core::LocalDefId::from_raw(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_raw(vec![block0, block1, block2]);
        body.locals = glyim_core::IndexVec::from_raw(
            (0..3)
                .map(|_| LocalDecl {
                    ty: int_ty,
                    mutability: Mutability::Not,
                    source_info: SourceInfo::new(glyim_span::Span::DUMMY),
                })
                .collect(),
        );
        body
    });
    crate::constant_prop::run(&ctx, &mut body);
    let block1 = &body.basic_blocks[BasicBlockIdx::from_raw(1)];
    if let StatementKind::Assign(_, Rvalue::BinaryOp(_, box_ops)) = &block1.statements[0].kind {
        assert!(
            matches!(box_ops[0], Operand::Constant(_)),
            "left operand in block1 should be constant propagated from block0"
        );
    } else {
        panic!("expected BinaryOp in block1");
    }
}

#[test]
fn bool_constant_propagation() {
    let (ctx, mut body) = with_fresh_ty_ctx(|_ctx_mut| {
        let bool_ty = Ty::BOOL;
        let const_true = MirConst {
            kind: MirConstKind::Bool(true),
            ty: bool_ty,
            span: glyim_span::Span::DUMMY,
        };
        let stmts = vec![
            Statement {
                kind: StatementKind::Assign(
                    make_place(1),
                    Rvalue::Use(Operand::Constant(const_true)),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(make_place(2), Rvalue::Use(make_copy(1))),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
        ];
        let block = BasicBlockData {
            statements: stmts,
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        };
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(0),
            glyim_core::LocalDefId::from_raw(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_raw(vec![block]);
        body.locals = glyim_core::IndexVec::from_raw(
            (0..3)
                .map(|_| LocalDecl {
                    ty: bool_ty,
                    mutability: Mutability::Not,
                    source_info: SourceInfo::new(glyim_span::Span::DUMMY),
                })
                .collect(),
        );
        body
    });
    crate::constant_prop::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_raw(0)];
    if let StatementKind::Assign(_, Rvalue::Use(op)) = &block.statements[1].kind {
        assert!(
            matches!(op, Operand::Constant(_)),
            "second assignment should be constant true"
        );
    } else {
        panic!("expected Use");
    }
}
