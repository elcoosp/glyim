//! Stress tests for Body construction and manipulation

use crate::*;
use glyim_span::Span;

#[test]
fn test_large_body_construction() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();

    for i in 0..1000 {
        let terminator = if i == 999 {
            TerminatorKind::Return
        } else {
            TerminatorKind::Goto {
                target: BasicBlockIdx::from_raw(i + 1),
            }
        };

        basic_blocks.push(BasicBlockData::new(Terminator {
            kind: terminator,
            source_info: SourceInfo::new(Span::DUMMY),
        }));
    }

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    for _ in 0..100 {
        locals.push(LocalDecl {
            ty: Ty::BOOL,
            mutability: glyim_core::primitives::Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
    }

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    assert_eq!(body.basic_blocks.len(), 1000);
    assert_eq!(body.locals.len(), 100);
}

#[test]
fn test_complex_switch_targets() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();

    let mut branches = Vec::new();
    for i in 0..50 {
        let bb = basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }));
        branches.push((i as u128, bb));
    }

    let otherwise = basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Unreachable,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let targets = SwitchTargets::new(branches.into_boxed_slice(), otherwise);

    let _switch_bb = basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Constant(MirConst {
                kind: MirConstKind::Int(25),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            }),
            switch_ty: Ty::ERROR,
            targets,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let body = Body {
        owner,
        basic_blocks,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    assert_eq!(body.basic_blocks.len(), 52);
}

#[test]
fn test_deeply_nested_terminators() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();

    let _prev_bb = BasicBlockIdx::from_raw(0);
    for i in 0..100 {
        let next_bb = basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_raw(i + 1),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        }));
        let _ = next_bb;
    }

    basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let body = Body {
        owner,
        basic_blocks,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    assert_eq!(body.basic_blocks.len(), 101);
}

#[test]
fn test_many_locals() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();

    basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    for i in 0..10000 {
        let ty = if i % 3 == 0 {
            Ty::BOOL
        } else if i % 3 == 1 {
            Ty::UNIT
        } else {
            Ty::NEVER
        };
        locals.push(LocalDecl {
            ty,
            mutability: glyim_core::primitives::Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
    }

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 1000,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    assert_eq!(body.locals.len(), 10000);
    assert_eq!(body.args().len(), 1000);
}

#[test]
fn test_mixed_projection_chains() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();

    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: glyim_core::primitives::Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(glyim_type::FieldIdx::from_raw(1)),
            ProjectionElem::Index(LocalIdx::from_raw(2)),
        ]),
    };

    let stmt = Statement {
        kind: StatementKind::Assign(
            place,
            Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(0)))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let _bb0 = basic_blocks.push(BasicBlockData {
        statements: vec![stmt],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
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

    assert_eq!(body.basic_blocks.len(), 1);
}

#[test]
fn test_large_aggregate_construction() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
    let locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();

    let mut operands = Vec::new();
    for i in 0..100 {
        operands.push(Operand::Constant(MirConst {
            kind: MirConstKind::Int(i),
            ty: Ty::ERROR,
            span: Span::DUMMY,
        }));
    }

    let rvalue = Rvalue::Aggregate(AggregateKind::Tuple, operands);
    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(0)), rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let _bb0 = basic_blocks.push(BasicBlockData {
        statements: vec![stmt],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
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

    assert_eq!(body.basic_blocks.len(), 1);
}
