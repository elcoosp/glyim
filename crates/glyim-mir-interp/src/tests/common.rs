use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Ty, TyCtxMut, TyKind};

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

fn local_decl(ty: Ty, mutability: Mutability) -> LocalDecl {
    LocalDecl {
        ty,
        mutability,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Build a body that computes lhs + rhs and stores in local 1.
pub fn build_add_body(_tcx: &TyCtxMut, lhs: i128, rhs: i128, ty: Ty) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(ty, Mutability::Mut),
    ]);
    let c1 = MirConst {
        kind: MirConstKind::Int(lhs),
        ty,
        span: Span::DUMMY,
    };
    let c2 = MirConst {
        kind: MirConstKind::Int(rhs),
        ty,
        span: Span::DUMMY,
    };
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(res_local),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new((Operand::Constant(c1), Operand::Constant(c2))),
            ),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![stmt],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    body
}

/// Build a body that computes lhs - rhs and stores in local 1.
pub fn build_sub_body(_tcx: &TyCtxMut, lhs: i128, rhs: i128, ty: Ty) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(ty, Mutability::Mut),
    ]);
    let c1 = MirConst {
        kind: MirConstKind::Int(lhs),
        ty,
        span: Span::DUMMY,
    };
    let c2 = MirConst {
        kind: MirConstKind::Int(rhs),
        ty,
        span: Span::DUMMY,
    };
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(res_local),
            Rvalue::BinaryOp(
                BinOp::Sub,
                Box::new((Operand::Constant(c1), Operand::Constant(c2))),
            ),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![stmt],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    body
}

/// Build a body that branches on a boolean constant.
/// If cond is true, `then_bb` is executed (returns), `else_bb` is unreachable.
/// If cond is false, `then_bb` is unreachable, `else_bb` is executed (returns).
pub fn build_branch_body(cond: bool, then_unreachable: bool, else_unreachable: bool) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    let bool_ty = Ty::BOOL;
    let discr_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(bool_ty, Mutability::Not),
    ]);
    let assign_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(discr_local),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Bool(cond),
                ty: bool_ty,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let then_target = BasicBlockIdx::from_raw(1);
    let else_target = BasicBlockIdx::from_raw(2);
    let switch_targets =
        SwitchTargets::new(vec![(0u128, else_target)].into_boxed_slice(), then_target);
    let switch = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(discr_local)),
            switch_ty: bool_ty,
            targets: switch_targets,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let then_terminator = if then_unreachable {
        Terminator {
            kind: TerminatorKind::Unreachable,
            source_info: SourceInfo::new(Span::DUMMY),
        }
    } else {
        Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }
    };
    let else_terminator = if else_unreachable {
        Terminator {
            kind: TerminatorKind::Unreachable,
            source_info: SourceInfo::new(Span::DUMMY),
        }
    } else {
        Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }
    };
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![assign_stmt],
            terminator: switch,
            is_cleanup: false,
        },
        BasicBlockData {
            statements: vec![],
            terminator: then_terminator,
            is_cleanup: false,
        },
        BasicBlockData {
            statements: vec![],
            terminator: else_terminator,
            is_cleanup: false,
        },
    ]);
    body
}

/// Build an infinite loop body (Goto to itself).
pub fn build_infinite_loop_body() -> Body {
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_raw(0),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    body
}

/// Build a recursive body: calls itself.
pub fn build_recursive_body(def_id: DefId) -> Body {
    let mut body = Body::dummy(def_id);
    body.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
    let call_terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Int(0), // placeholder, function value not representable as const
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(LocalIdx::from_raw(0)),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![],
            terminator: call_terminator,
            is_cleanup: false,
        },
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
    ]);
    body
}

/// Build a body with an unreachable terminator.
pub fn build_unreachable_body() -> Body {
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Unreachable,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    body
}

/// Build a body with `StorageLive`, assign, `StorageDead`.
pub fn build_allocation_body(tcx: &mut TyCtxMut, val: i128) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    let local = LocalIdx::from_raw(1);
    let ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(ty, Mutability::Mut),
    ]);
    let c = MirConst {
        kind: MirConstKind::Int(val),
        ty,
        span: Span::DUMMY,
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![
            Statement {
                kind: StatementKind::StorageLive(local),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(Place::new(local), Rvalue::Use(Operand::Constant(c))),
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    body
}
