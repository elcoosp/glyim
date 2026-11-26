use glyim_core::Mutability;
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;

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

#[test]
fn eliminates_unreachable_block() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = ctx_mut.mk_ty(glyim_type::TyKind::Unit);
        let block0 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
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
    crate::unreachable_elim::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        1,
        "should remove unreachable block"
    );
}

#[test]
fn all_reachable_no_change() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = ctx_mut.mk_ty(glyim_type::TyKind::Unit);
        let block0 = BasicBlockData {
            statements: vec![],
            terminator: goto_term(1),
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
    crate::unreachable_elim::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        2,
        "all reachable, no change expected"
    );
}

#[test]
fn multiple_unreachable_removed() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = ctx_mut.mk_ty(glyim_type::TyKind::Unit);
        // block0: return; block1: unreachable; block2: unreachable
        let block0 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
        let block2 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
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
    crate::unreachable_elim::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        1,
        "should remove both unreachable blocks"
    );
}

#[test]
fn switchint_keeps_reachable() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = ctx_mut.mk_ty(glyim_type::TyKind::Unit);
        let block0 = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_usize(0))),
                    switch_ty: ut,
                    targets: SwitchTargets::new(
                        vec![
                            (0, BasicBlockIdx::from_usize(1)),
                            (1, BasicBlockIdx::from_usize(2)),
                        ]
                        .into_boxed_slice(),
                        BasicBlockIdx::from_usize(2),
                    ),
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
        let block2 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
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
    crate::unreachable_elim::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        3,
        "all three blocks reachable via SwitchInt"
    );
}

#[test]
fn transitive_unreachable_removed() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = ctx_mut.mk_ty(glyim_type::TyKind::Unit);
        // block0: return
        // block1: goto block2 (unreachable)
        // block2: return (also unreachable)
        let block0 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![],
            terminator: goto_term(2),
            is_cleanup: false,
        };
        let block2 = BasicBlockData {
            statements: vec![],
            terminator: return_term(),
            is_cleanup: false,
        };
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
    crate::unreachable_elim::run(&ctx, &mut body);
    assert_eq!(
        body.basic_blocks.len(),
        1,
        "only the first block should remain, transitive unreachable removed"
    );
}

#[test]
fn cycle_all_reachable() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx_mut| {
        let ut = ctx_mut.mk_ty(glyim_type::TyKind::Unit);
        // block0: goto block1
        // block1: goto block0 (cycle)
        let block0 = BasicBlockData {
            statements: vec![],
            terminator: goto_term(1),
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![],
            terminator: goto_term(0),
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
    crate::unreachable_elim::run(&ctx, &mut body);
    assert_eq!(body.basic_blocks.len(), 2, "cycle blocks remain reachable");
}
