use crate::*;
use glyim_span::Span;
use glyim_type::Ty;

#[test]
fn basic_block_data_new_with_terminator() {
    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let bb = BasicBlockData::new(term);

    assert!(bb.statements.is_empty());
    assert!(!bb.is_cleanup);
    match &bb.terminator.kind {
        TerminatorKind::Return => {}
        other => panic!("Expected Return, got {other:?}"),
    }
}

#[test]
fn basic_block_data_with_statements() {
    let term = Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_raw(1),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let mut bb = BasicBlockData::new(term);

    bb.statements.push(Statement {
        kind: StatementKind::StorageLive(LocalIdx::from_raw(1)),
        source_info: SourceInfo::new(Span::DUMMY),
    });
    bb.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(42),
                ty: Ty::BOOL,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    });
    bb.statements.push(Statement {
        kind: StatementKind::StorageDead(LocalIdx::from_raw(1)),
        source_info: SourceInfo::new(Span::DUMMY),
    });

    assert_eq!(bb.statements.len(), 3);
    assert!(matches!(
        bb.statements[0].kind,
        StatementKind::StorageLive(_)
    ));
    assert!(matches!(bb.statements[1].kind, StatementKind::Assign(_, _)));
    assert!(matches!(
        bb.statements[2].kind,
        StatementKind::StorageDead(_)
    ));
}

#[test]
fn basic_block_data_cleanup_flag() {
    let term = Terminator {
        kind: TerminatorKind::Unreachable,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let mut bb = BasicBlockData::new(term);
    assert!(!bb.is_cleanup);

    bb.is_cleanup = true;
    assert!(bb.is_cleanup);
}
