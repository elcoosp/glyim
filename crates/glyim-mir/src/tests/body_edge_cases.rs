use crate::*;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_span::Span;
use glyim_type::Ty;

fn si() -> SourceInfo {
    SourceInfo::new(Span::DUMMY)
}

#[test]
fn body_args_with_no_args_returns_empty() {
    let body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    assert!(body.args().is_empty());
    assert_eq!(body.args().len(), 0);
}

#[test]
fn body_args_with_one_arg() {
    let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    body.locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: si(),
    });
    body.arg_count = 1;
    assert_eq!(body.args().len(), 1);
    assert_eq!(body.args()[0].ty, Ty::BOOL);
}

#[test]
fn body_args_with_multiple_args() {
    let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    body.locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: si(),
    });
    body.locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: si(),
    });
    body.locals.push(LocalDecl {
        ty: Ty::NEVER,
        mutability: Mutability::Not,
        source_info: si(),
    });
    body.arg_count = 3;
    assert_eq!(body.args().len(), 3);
    assert_eq!(body.args()[0].ty, Ty::BOOL);
    assert_eq!(body.args()[1].ty, Ty::UNIT);
    assert_eq!(body.args()[2].ty, Ty::NEVER);
}

#[test]
fn body_return_place_is_local_zero() {
    let body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    let rp = body.return_place();
    assert_eq!(rp.local, LocalIdx::from_raw(0));
    assert!(rp.projection.is_empty());
}

#[test]
fn body_dummy_span_is_dummy() {
    let body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    assert!(body.span.is_dummy());
}

#[test]
fn body_multiple_blocks_with_statements() {
    let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));

    body.locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Mut,
        source_info: si(),
    });

    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_raw(1),
        },
        source_info: si(),
    });
    bb0.statements.push(Statement {
        kind: StatementKind::StorageLive(LocalIdx::from_raw(1)),
        source_info: si(),
    });
    body.basic_blocks.push(bb0);

    let mut bb1 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: si(),
    });
    bb1.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Bool(true),
                ty: Ty::BOOL,
                span: Span::DUMMY,
            })),
        ),
        source_info: si(),
    });
    bb1.statements.push(Statement {
        kind: StatementKind::StorageDead(LocalIdx::from_raw(1)),
        source_info: si(),
    });
    body.basic_blocks.push(bb1);

    assert_eq!(body.basic_blocks.len(), 3);
    assert_eq!(
        body.basic_blocks[BasicBlockIdx::from_raw(1)]
            .statements
            .len(),
        1
    );
    assert_eq!(
        body.basic_blocks[BasicBlockIdx::from_raw(2)]
            .statements
            .len(),
        2
    );
}

#[test]
fn body_clone() {
    let body = Body::dummy(DefId::new(CrateId::from_raw(5), LocalDefId::from_raw(10)));
    let cloned = body.clone();
    assert_eq!(body.owner.krate, cloned.owner.krate);
    assert_eq!(body.owner.local_id, cloned.owner.local_id);
    assert_eq!(body.arg_count, cloned.arg_count);
    assert_eq!(body.return_ty, cloned.return_ty);
}

#[test]
fn body_debug_format() {
    let body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    let debug_str = format!("{body:?}");
    assert!(debug_str.contains("Body"));
}
