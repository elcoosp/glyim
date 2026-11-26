use crate::*;
use glyim_span::Span;
use glyim_type::Ty;

#[test]
fn statement_assign() {
    let place = Place::new(LocalIdx::from_usize(1));
    let value = Rvalue::Use(Operand::Constant(MirConst {
        kind: MirConstKind::Int(10),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    }));
    let stmt = Statement {
        kind: StatementKind::Assign(place, value),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    assert!(matches!(stmt.kind, StatementKind::Assign(_, _)));
}

#[test]
fn statement_storage_live() {
    let local = LocalIdx::from_usize(5);
    let stmt = Statement {
        kind: StatementKind::StorageLive(local),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    assert!(matches!(stmt.kind, StatementKind::StorageLive(l) if l == local));
}

#[test]
fn statement_storage_dead() {
    let local = LocalIdx::from_usize(5);
    let stmt = Statement {
        kind: StatementKind::StorageDead(local),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    assert!(matches!(stmt.kind, StatementKind::StorageDead(l) if l == local));
}

#[test]
fn statement_nop() {
    let stmt = Statement {
        kind: StatementKind::Nop,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    assert!(matches!(stmt.kind, StatementKind::Nop));
}
