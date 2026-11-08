use crate::*;
use glyim_core::primitives::BinOp;
use glyim_span::Span;
use glyim_type::Ty;

fn si() -> SourceInfo {
    SourceInfo::new(Span::DUMMY)
}

#[test]
fn call_no_target_unwinding() {
    let term = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Uint(0),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(LocalIdx::from_raw(1)),
            target: None,
            cleanup: Some(BasicBlockIdx::from_raw(5)),
        },
        source_info: si(),
    };
    match &term.kind {
        TerminatorKind::Call {
            target: None,
            cleanup: Some(cb),
            ..
        } => {
            assert_eq!(*cb, BasicBlockIdx::from_raw(5));
        }
        other => panic!("Expected Call with no target, got {other:?}"),
    }
}

#[test]
fn call_no_target_no_cleanup() {
    let term = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Uint(0),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(LocalIdx::from_raw(1)),
            target: None,
            cleanup: None,
        },
        source_info: si(),
    };
    assert!(matches!(
        term.kind,
        TerminatorKind::Call {
            target: None,
            cleanup: None,
            ..
        }
    ));
}

#[test]
fn call_with_multiple_args() {
    let arg1 = Operand::Copy(Place::new(LocalIdx::from_raw(1)));
    let arg2 = Operand::Move(Place::new(LocalIdx::from_raw(2)));
    let arg3 = Operand::Constant(MirConst {
        kind: MirConstKind::Bool(false),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    });

    let term = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Uint(0),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![arg1, arg2, arg3],
            destination: Place::new(LocalIdx::from_raw(3)),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: si(),
    };
    if let TerminatorKind::Call { args, .. } = &term.kind {
        assert_eq!(args.len(), 3);
    } else {
        panic!("Expected Call");
    }
}

#[test]
fn assert_all_message_types() {
    let msgs = vec![
        AssertMessage::Overflow(BinOp::Add),
        AssertMessage::Overflow(BinOp::Sub),
        AssertMessage::Overflow(BinOp::Mul),
        AssertMessage::DivisionByZero,
        AssertMessage::RemainderByZero,
        AssertMessage::BoundsCheck,
    ];
    for msg in msgs {
        let term = Terminator {
            kind: TerminatorKind::Assert {
                cond: Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(true),
                    ty: Ty::BOOL,
                    span: Span::DUMMY,
                }),
                expected: true,
                target: BasicBlockIdx::from_raw(0),
                cleanup: None,
                msg,
            },
            source_info: si(),
        };
        assert!(matches!(term.kind, TerminatorKind::Assert { .. }));
    }
}

#[test]
fn drop_with_cleanup() {
    let term = Terminator {
        kind: TerminatorKind::Drop {
            place: Place::new(LocalIdx::from_raw(2)),
            target: BasicBlockIdx::from_raw(3),
            cleanup: Some(BasicBlockIdx::from_raw(4)),
        },
        source_info: si(),
    };
    if let TerminatorKind::Drop {
        target, cleanup, ..
    } = &term.kind
    {
        assert_eq!(*target, BasicBlockIdx::from_raw(3));
        assert_eq!(*cleanup, Some(BasicBlockIdx::from_raw(4)));
    } else {
        panic!("Expected Drop");
    }
}

#[test]
fn switch_int_with_many_targets() {
    let targets = SwitchTargets::new(
        Box::new([
            (0u128, BasicBlockIdx::from_raw(1)),
            (1u128, BasicBlockIdx::from_raw(2)),
            (2u128, BasicBlockIdx::from_raw(3)),
            (100u128, BasicBlockIdx::from_raw(4)),
        ]),
        BasicBlockIdx::from_raw(5),
    );
    let term = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(LocalIdx::from_raw(0))),
            switch_ty: Ty::BOOL,
            targets,
        },
        source_info: si(),
    };
    if let TerminatorKind::SwitchInt { targets, .. } = &term.kind {
        assert_eq!(targets.iter().count(), 4);
        assert_eq!(targets.otherwise(), BasicBlockIdx::from_raw(5));
    } else {
        panic!("Expected SwitchInt");
    }
}
