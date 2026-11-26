use glyim_core::Mutability;
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;

fn unit_ty(ctx: &mut glyim_type::TyCtxMut) -> glyim_type::Ty {
    ctx.mk_ty(glyim_type::TyKind::Unit)
}
fn return_term() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    }
}
fn goto_term(t: u32) -> Terminator {
    Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_usize(t as usize),
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    }
}
fn nop_block() -> BasicBlockData {
    BasicBlockData {
        statements: vec![],
        terminator: return_term(),
        is_cleanup: false,
    }
}
fn make_place(idx: u32) -> Place {
    Place::new(LocalIdx::from_usize(idx as usize))
}
fn make_copy(idx: u32) -> Operand {
    Operand::Copy(make_place(idx))
}

fn make_body_double_goto(_ctx: &mut glyim_type::TyCtxMut) -> Body {
    let block0 = BasicBlockData {
        statements: vec![],
        terminator: goto_term(1),
        is_cleanup: false,
    };
    let block1 = BasicBlockData {
        statements: vec![],
        terminator: goto_term(2),
        is_cleanup: false,
    };
    let block2 = nop_block();
    let mut body = Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_usize(0),
        glyim_core::LocalDefId::from_usize(0),
    ));
    body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1, block2]);
    body.locals = glyim_core::IndexVec::new();
    body
}

fn make_body_diamond(ctx: &mut glyim_type::TyCtxMut) -> Body {
    let _ut = unit_ty(ctx);
    let block0 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: make_copy(0),
                switch_ty: _ut,
                targets: SwitchTargets::new(
                    vec![
                        (0, BasicBlockIdx::from_usize(1)),
                        (1, BasicBlockIdx::from_usize(2)),
                    ]
                    .into_boxed_slice(),
                    BasicBlockIdx::from_usize(3),
                ),
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: false,
    };
    let block1 = BasicBlockData {
        statements: vec![],
        terminator: goto_term(3),
        is_cleanup: false,
    };
    let block2 = BasicBlockData {
        statements: vec![],
        terminator: goto_term(3),
        is_cleanup: false,
    };
    let block3 = nop_block();
    let mut body = Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_usize(0),
        glyim_core::LocalDefId::from_usize(0),
    ));
    body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1, block2, block3]);
    body.locals = glyim_core::IndexVec::from_vec(vec![LocalDecl {
        ty: _ut,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    }]);
    body
}

fn make_body_loop(ctx: &mut glyim_type::TyCtxMut) -> Body {
    let _ut = unit_ty(ctx);
    let block0 = BasicBlockData {
        statements: vec![],
        terminator: goto_term(1),
        is_cleanup: false,
    };
    let block1 = BasicBlockData {
        statements: vec![],
        terminator: goto_term(1),
        is_cleanup: false,
    };
    let mut body = Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_usize(0),
        glyim_core::LocalDefId::from_usize(0),
    ));
    body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
    body.locals = glyim_core::IndexVec::new();
    body
}

#[test]
fn merges_single_goto_chain() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = unit_ty(ctx_mut);
        let block0 = BasicBlockData {
            statements: vec![],
            terminator: goto_term(1),
            is_cleanup: false,
        };
        let stmt = Statement {
            kind: StatementKind::Assign(
                make_place(0),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Unit,
                    ty: ut,
                    span: glyim_span::Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };
        let term = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: make_copy(0),
                switch_ty: ut,
                targets: SwitchTargets::new(
                    vec![(0, BasicBlockIdx::from_usize(2))].into_boxed_slice(),
                    BasicBlockIdx::from_usize(2),
                ),
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };
        let block1 = BasicBlockData {
            statements: vec![stmt],
            terminator: term,
            is_cleanup: false,
        };
        let block2 = nop_block();
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1, block2]);
        body.locals = glyim_core::IndexVec::from_vec(vec![LocalDecl {
            ty: ut,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }]);
        body
    });
    crate::cfg_simplify::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        2,
        "should merge block0 into block1, leaving 2 blocks"
    );
    assert!(
        !body.basic_blocks[BasicBlockIdx::from_usize(0)]
            .statements
            .is_empty(),
        "merged block should contain statement"
    );
}

#[test]
fn merges_double_goto_chain() {
    let (ctx, mut body) = with_fresh_ty_ctx(make_body_double_goto);
    crate::cfg_simplify::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        1,
        "double goto chain should merge into a single block"
    );
    assert!(matches!(
        body.basic_blocks[BasicBlockIdx::from_usize(0)]
            .terminator
            .kind,
        TerminatorKind::Return
    ));
}

#[test]
fn diamond_not_merged() {
    let (ctx, mut body) = with_fresh_ty_ctx(make_body_diamond);
    let original_len = body.basic_blocks.len();
    crate::cfg_simplify::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        original_len,
        "diamond should not be modified"
    );
}

#[test]
fn self_loop_not_merged() {
    let (ctx, mut body) = with_fresh_ty_ctx(make_body_loop);
    let original_len = body.basic_blocks.len();
    crate::cfg_simplify::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        original_len,
        "loop should not be merged"
    );
}

#[test]
fn call_terminator_not_merged() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = unit_ty(ctx_mut);
        let block0 = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Call {
                    func: make_copy(0),
                    args: vec![],
                    destination: make_place(0),
                    target: Some(BasicBlockIdx::from_usize(1)),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
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
        body.locals = glyim_core::IndexVec::from_vec(vec![LocalDecl {
            ty: ut,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }]);
        body
    });
    let original_len = body.basic_blocks.len();
    crate::cfg_simplify::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        original_len,
        "Call terminator should not trigger merge"
    );
}

#[test]
fn switchint_single_target_not_merged() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = unit_ty(ctx_mut);
        let block0 = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: make_copy(0),
                    switch_ty: ut,
                    targets: SwitchTargets::new(
                        vec![(0, BasicBlockIdx::from_usize(1))].into_boxed_slice(),
                        BasicBlockIdx::from_usize(1),
                    ),
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        };
        let block1 = nop_block();
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_vec(vec![block0, block1]);
        body.locals = glyim_core::IndexVec::from_vec(vec![LocalDecl {
            ty: ut,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }]);
        body
    });
    let original_len = body.basic_blocks.len();
    crate::cfg_simplify::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        original_len,
        "SwitchInt with single target should not merge"
    );
}

#[test]
fn merge_remaps_indices_correctly() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = unit_ty(ctx_mut);
        // block0: goto block1
        // block1: goto block2
        // block2: goto block3
        // block3: return
        let blocks = (0..4)
            .map(|i| {
                let term = if i < 3 {
                    Terminator {
                        kind: TerminatorKind::Goto {
                            target: BasicBlockIdx::from_usize(i + 1),
                        },
                        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
                    }
                } else {
                    return_term()
                };
                BasicBlockData {
                    statements: vec![],
                    terminator: term,
                    is_cleanup: false,
                }
            })
            .collect::<Vec<_>>();
        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ));
        body.basic_blocks = glyim_core::IndexVec::from_vec(blocks);
        body.locals = glyim_core::IndexVec::from_vec(vec![LocalDecl {
            ty: ut,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }]);
        body
    });
    crate::cfg_simplify::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        1,
        "all goto chains should merge into one block"
    );
    assert!(
        matches!(
            body.basic_blocks[BasicBlockIdx::from_usize(0)]
                .terminator
                .kind,
            TerminatorKind::Return
        ),
        "final terminator should be Return"
    );
}
