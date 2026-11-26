use glyim_core::Mutability;
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;

fn make_int_ty(ctx: &mut glyim_type::TyCtxMut) -> glyim_type::Ty {
    ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32))
}
fn make_place(idx: u32) -> Place {
    Place::new(LocalIdx::from_usize(idx as usize))
}
fn make_copy(idx: u32) -> Operand {
    Operand::Copy(make_place(idx))
}
fn make_const(val: i128, ty: glyim_type::Ty) -> MirConst {
    MirConst {
        kind: MirConstKind::Int(val),
        ty,
        span: glyim_span::Span::DUMMY,
    }
}
fn return_term() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    }
}

fn make_body_multi_dead(ctx: &mut glyim_type::TyCtxMut) -> Body {
    let int_ty = make_int_ty(ctx);
    let stmts = vec![
        Statement {
            kind: StatementKind::Assign(
                make_place(1),
                Rvalue::Use(Operand::Constant(make_const(1, int_ty))),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                make_place(2),
                Rvalue::Use(Operand::Constant(make_const(2, int_ty))),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                make_place(3),
                Rvalue::BinaryOp(glyim_core::BinOp::Add, [make_copy(0), make_copy(0)]),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
    ];
    let term = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: make_copy(3),
            switch_ty: int_ty,
            targets: SwitchTargets::new(
                vec![(0, BasicBlockIdx::from_usize(1))].into_boxed_slice(),
                BasicBlockIdx::from_usize(1),
            ),
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    let block0 = BasicBlockData {
        statements: stmts,
        terminator: term,
        is_cleanup: false,
    };
    let block1 = BasicBlockData {
        statements: vec![],
        terminator: return_term(),
        is_cleanup: false,
    };
    let mut body = Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_usize(0),
        glyim_core::LocalDefId::from_usize(0),
    ));
    body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
    body.locals = glyim_core::IndexVec::from_vec(
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

fn make_body_all_live(ctx: &mut glyim_type::TyCtxMut) -> Body {
    let int_ty = make_int_ty(ctx);
    let stmts = vec![
        Statement {
            kind: StatementKind::Assign(
                make_place(1),
                Rvalue::Use(Operand::Constant(make_const(5, int_ty))),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(make_place(2), Rvalue::Use(make_copy(1))),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
    ];
    let term = Terminator {
        kind: TerminatorKind::Call {
            func: make_copy(1),
            args: vec![make_copy(2)],
            destination: make_place(0),
            target: Some(BasicBlockIdx::from_usize(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    let block0 = BasicBlockData {
        statements: stmts,
        terminator: term,
        is_cleanup: false,
    };
    let block1 = BasicBlockData {
        statements: vec![],
        terminator: return_term(),
        is_cleanup: false,
    };
    let mut body = Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_usize(0),
        glyim_core::LocalDefId::from_usize(0),
    ));
    body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
    body.locals = glyim_core::IndexVec::from_vec(
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

fn make_body_aggregate_use(ctx: &mut glyim_type::TyCtxMut) -> Body {
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
            kind: StatementKind::Assign(
                make_place(2),
                Rvalue::Use(Operand::Constant(make_const(20, int_ty))),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                make_place(3),
                Rvalue::Aggregate(AggregateKind::Tuple, vec![make_copy(1), make_copy(2)]),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
    ];
    let term = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: make_copy(3),
            switch_ty: int_ty,
            targets: SwitchTargets::new(
                vec![(0, BasicBlockIdx::from_usize(1))].into_boxed_slice(),
                BasicBlockIdx::from_usize(1),
            ),
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    let block0 = BasicBlockData {
        statements: stmts,
        terminator: term,
        is_cleanup: false,
    };
    let block1 = BasicBlockData {
        statements: vec![],
        terminator: return_term(),
        is_cleanup: false,
    };
    let mut body = Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_usize(0),
        glyim_core::LocalDefId::from_usize(0),
    ));
    body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
    body.locals = glyim_core::IndexVec::from_vec(
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

// Helper to extract assigned place from StatementKind
fn assign_place(kind: &StatementKind) -> Option<&Place> {
    match kind {
        StatementKind::Assign(p, _) => Some(p),
        _ => None,
    }
}

#[test]
fn eliminates_single_dead_store() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let int_ty = make_int_ty(ctx_mut);
        let stmts = vec![
            Statement {
                kind: StatementKind::Assign(
                    make_place(1),
                    Rvalue::Use(Operand::Constant(make_const(1, int_ty))),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    make_place(2),
                    Rvalue::BinaryOp(glyim_core::BinOp::Add, [make_copy(0), make_copy(0)]),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
        ];
        let term = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: make_copy(2),
                switch_ty: int_ty,
                targets: SwitchTargets::new(
                    vec![(0, BasicBlockIdx::from_usize(1))].into_boxed_slice(),
                    BasicBlockIdx::from_usize(1),
                ),
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };
        let block0 = BasicBlockData {
            statements: stmts,
            terminator: term,
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
        body.locals = glyim_core::IndexVec::from_vec(
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
    crate::dce::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_usize(0)];
    assert_eq!(
        block.statements.len(),
        1,
        "dead statement should be removed, live should stay"
    );
    assert_eq!(
        assign_place(&block.statements[0].kind)
            .unwrap()
            .local
            .to_raw(),
        2
    );
}

#[test]
fn removes_multiple_dead_stores() {
    let (ctx, mut body) = with_fresh_ty_ctx(make_body_multi_dead);
    crate::dce::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_usize(0)];
    assert_eq!(
        block.statements.len(),
        1,
        "two dead stores should be removed, one live remains"
    );
    assert_eq!(
        assign_place(&block.statements[0].kind)
            .unwrap()
            .local
            .to_raw(),
        3
    );
}

#[test]
fn keeps_live_stores_with_aggregate_use() {
    let (ctx, mut body) = with_fresh_ty_ctx(make_body_aggregate_use);
    crate::dce::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_usize(0)];
    assert_eq!(
        block.statements.len(),
        3,
        "all stores should be live due to aggregate use chain"
    );
}

#[test]
fn all_live_preserves_everything() {
    let (ctx, mut body) = with_fresh_ty_ctx(make_body_all_live);
    crate::dce::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_usize(0)];
    assert_eq!(
        block.statements.len(),
        2,
        "both stores should be preserved since both are live"
    );
}

#[test]
fn storage_live_dead_kept() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let int_ty = make_int_ty(ctx_mut);
        let stmts = vec![
            Statement {
                kind: StatementKind::StorageLive(LocalIdx::from_usize(1)),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    make_place(1),
                    Rvalue::Use(Operand::Constant(make_const(1, int_ty))),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::StorageDead(LocalIdx::from_usize(1)),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            // live assignment to local2 that is used in terminator
            Statement {
                kind: StatementKind::Assign(
                    make_place(2),
                    Rvalue::BinaryOp(glyim_core::BinOp::Add, [make_copy(0), make_copy(0)]),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
        ];
        let term = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: make_copy(2),
                switch_ty: int_ty,
                targets: SwitchTargets::new(
                    vec![(0, BasicBlockIdx::from_usize(1))].into_boxed_slice(),
                    BasicBlockIdx::from_usize(1),
                ),
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };
        let block0 = BasicBlockData {
            statements: stmts,
            terminator: term,
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
        body.locals = glyim_core::IndexVec::from_vec(
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
    crate::dce::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_usize(0)];
    assert_eq!(
        block.statements.len(),
        3,
        "StorageLive/Dead and live assignment retained, dead assignment removed"
    );
    // The statements: [StorageLive(1), StorageDead(1), liveAssignment]
    assert!(matches!(
        block.statements[0].kind,
        StatementKind::StorageLive(_)
    ));
    assert!(matches!(
        block.statements[1].kind,
        StatementKind::StorageDead(_)
    ));
    assert!(matches!(
        block.statements[2].kind,
        StatementKind::Assign(..)
    ));
}

#[test]
fn cast_operand_keeps_assignment_live() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let int_ty = make_int_ty(ctx_mut);
        let i64_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I64));
        let stmts = vec![
            Statement {
                kind: StatementKind::Assign(
                    make_place(1),
                    Rvalue::Use(Operand::Constant(make_const(5, int_ty))),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    make_place(2),
                    Rvalue::Cast(glyim_mir::CastKind::IntToInt, make_copy(1), i64_ty),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
        ];
        let term = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: make_copy(2),
                switch_ty: i64_ty,
                targets: SwitchTargets::new(
                    vec![(0, BasicBlockIdx::from_usize(1))].into_boxed_slice(),
                    BasicBlockIdx::from_usize(1),
                ),
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };
        let block0 = BasicBlockData {
            statements: stmts,
            terminator: term,
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
        body.locals = glyim_core::IndexVec::from_vec(
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
    crate::dce::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_usize(0)];
    assert_eq!(
        block.statements.len(),
        2,
        "both assignment to local1 (used in cast) and cast remain"
    );
}

#[test]
fn ref_operand_keeps_assignment_live() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let int_ty = make_int_ty(ctx_mut);
        let ref_ty = ctx_mut.mk_ref(glyim_type::Region::Erased, int_ty, Mutability::Not);
        let stmts = vec![
            Statement {
                kind: StatementKind::Assign(
                    make_place(1),
                    Rvalue::Use(Operand::Constant(make_const(5, int_ty))),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    make_place(2),
                    Rvalue::Ref(make_place(1), glyim_mir::BorrowKind::Shared),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
        ];
        let term = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: make_copy(2),
                switch_ty: ref_ty,
                targets: SwitchTargets::new(
                    vec![(0, BasicBlockIdx::from_usize(1))].into_boxed_slice(),
                    BasicBlockIdx::from_usize(1),
                ),
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };
        let block0 = BasicBlockData {
            statements: stmts,
            terminator: term,
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
        body.locals = glyim_core::IndexVec::from_vec(
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
    crate::dce::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_usize(0)];
    assert_eq!(
        block.statements.len(),
        2,
        "both assignment to local1 (used in ref) and ref remain"
    );
}

#[test]
fn dead_store_with_live_operands() {
    // local2 = 10 (live because used in local3); local1 = local2 (dead); local3 = local2 + 1 (live)
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let int_ty = make_int_ty(ctx_mut);
        let stmts = vec![
            Statement {
                kind: StatementKind::Assign(
                    make_place(2),
                    Rvalue::Use(Operand::Constant(make_const(10, int_ty))),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(make_place(1), Rvalue::Use(make_copy(2))),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    make_place(3),
                    Rvalue::BinaryOp(
                        glyim_core::BinOp::Add,
                        [make_copy(2), Operand::Constant(make_const(1, int_ty))],
                    ),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
        ];
        let term = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: make_copy(3),
                switch_ty: int_ty,
                targets: SwitchTargets::new(
                    vec![(0, BasicBlockIdx::from_usize(1))].into_boxed_slice(),
                    BasicBlockIdx::from_usize(1),
                ),
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };
        let block0 = BasicBlockData {
            statements: stmts,
            terminator: term,
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
        body.locals = glyim_core::IndexVec::from_vec(
            (0..4)
                .map(|_| LocalDecl {
                    ty: int_ty,
                    mutability: Mutability::Not,
                    source_info: SourceInfo::new(glyim_span::Span::DUMMY),
                })
                .collect(),
        );
        body
    });
    crate::dce::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_usize(0)];
    assert_eq!(
        block.statements.len(),
        2,
        "dead store to local1 removed, local2 and local3 assignments remain"
    );
    // Check that the remaining assignments are for local2 and local3
    let places: Vec<_> = block
        .statements
        .iter()
        .filter_map(|s| assign_place(&s.kind))
        .map(|p| p.local.to_raw())
        .collect();
    assert_eq!(places, vec![2, 3]);
}
