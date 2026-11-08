use crate::*;
use glyim_core::primitives::BinOp;
use glyim_span::Span;
use glyim_type::Ty;

#[test]
fn terminator_goto() {
    let target = BasicBlockIdx::from_raw(5);
    let term = Terminator {
        kind: TerminatorKind::Goto { target },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    match &term.kind {
        TerminatorKind::Goto { target: t } => assert_eq!(*t, target),
        other => panic!("Expected Goto, got {other:?}"),
    }
}

#[test]
fn terminator_return() {
    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    assert!(matches!(term.kind, TerminatorKind::Return));
}

#[test]
fn terminator_unreachable() {
    let term = Terminator {
        kind: TerminatorKind::Unreachable,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    assert!(matches!(term.kind, TerminatorKind::Unreachable));
}

#[test]
fn terminator_call_with_target() {
    let func = Operand::Constant(MirConst {
        kind: MirConstKind::Uint(0),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    });
    let dest = Place::new(LocalIdx::from_raw(1));
    let term = Terminator {
        kind: TerminatorKind::Call {
            func,
            args: vec![],
            destination: dest,
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    match &term.kind {
        TerminatorKind::Call {
            target: Some(bb),
            cleanup: None,
            ..
        } => assert_eq!(*bb, BasicBlockIdx::from_raw(1)),
        other => panic!("Expected Call, got {other:?}"),
    }
}

#[test]
fn terminator_call_with_cleanup() {
    let func = Operand::Constant(MirConst {
        kind: MirConstKind::Uint(0),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    });
    let dest = Place::new(LocalIdx::from_raw(1));
    let term = Terminator {
        kind: TerminatorKind::Call {
            func,
            args: vec![],
            destination: dest,
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: Some(BasicBlockIdx::from_raw(2)),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    match &term.kind {
        TerminatorKind::Call {
            target: Some(_),
            cleanup: Some(cleanup_bb),
            ..
        } => assert_eq!(*cleanup_bb, BasicBlockIdx::from_raw(2)),
        other => panic!("Expected Call with cleanup, got {other:?}"),
    }
}

#[test]
fn terminator_assert() {
    let cond = Operand::Constant(MirConst {
        kind: MirConstKind::Bool(true),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    });
    let term = Terminator {
        kind: TerminatorKind::Assert {
            cond,
            expected: true,
            target: BasicBlockIdx::from_raw(3),
            cleanup: Some(BasicBlockIdx::from_raw(4)),
            msg: AssertMessage::Overflow(BinOp::Add),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    match &term.kind {
        TerminatorKind::Assert {
            expected,
            target,
            msg,
            ..
        } => {
            assert!(*expected);
            assert_eq!(*target, BasicBlockIdx::from_raw(3));
            assert!(matches!(msg, AssertMessage::Overflow(BinOp::Add)));
        }
        other => panic!("Expected Assert, got {other:?}"),
    }
}

#[test]
fn terminator_drop() {
    let place = Place::new(LocalIdx::from_raw(2));
    let term = Terminator {
        kind: TerminatorKind::Drop {
            place,
            target: BasicBlockIdx::from_raw(1),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    match &term.kind {
        TerminatorKind::Drop {
            target,
            cleanup: None,
            ..
        } => assert_eq!(*target, BasicBlockIdx::from_raw(1)),
        other => panic!("Expected Drop, got {other:?}"),
    }
}

#[test]
fn terminator_switch_int() {
    let discr = Operand::Constant(MirConst {
        kind: MirConstKind::Uint(0),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    });
    let targets = SwitchTargets::if_switch(BasicBlockIdx::from_raw(2), BasicBlockIdx::from_raw(3));
    let term = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr,
            switch_ty: Ty::BOOL,
            targets,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    match &term.kind {
        TerminatorKind::SwitchInt {
            switch_ty, targets, ..
        } => {
            assert_eq!(*switch_ty, Ty::BOOL);
            assert_eq!(targets.otherwise(), BasicBlockIdx::from_raw(3));
        }
        other => panic!("Expected SwitchInt, got {other:?}"),
    }
}
