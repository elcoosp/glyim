use crate::*;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_span::Span;
use glyim_type::Ty;

#[test]
fn dummy_creates_valid_structure() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let body = Body::dummy(owner);

    assert_eq!(body.owner.krate, CrateId::from_raw(0));
    assert_eq!(body.owner.local_id, LocalDefId::from_raw(0));
    assert_eq!(body.arg_count, 0);
    assert_eq!(body.return_ty, Ty::UNIT);
    assert_eq!(body.basic_blocks.len(), 1);
    assert_eq!(body.locals.len(), 1);
    assert_eq!(body.locals[LocalIdx::from_raw(0)].ty, Ty::UNIT);
    assert!(body.var_debug_info.is_empty());
}

#[test]
fn dummy_has_unreachable_terminator() {
    let owner = DefId::new(CrateId::from_raw(1), LocalDefId::from_raw(2));
    let body = Body::dummy(owner);

    let bb0 = BasicBlockIdx::from_raw(0);
    match &body.basic_blocks[bb0].terminator.kind {
        TerminatorKind::Unreachable => {}
        other => panic!("Expected Unreachable, got {other:?}"),
    }
}

#[test]
fn dummy_return_place() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let body = Body::dummy(owner);

    let ret = body.return_place();
    assert_eq!(ret.local, LocalIdx::from_raw(0));
    assert!(ret.projection.is_empty());
}

#[test]
fn body_args_returns_correct_slice() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut body = Body::dummy(owner);

    let arg1 = LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let arg2 = LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body.locals.push(arg1);
    body.locals.push(arg2);
    body.arg_count = 2;

    let args = body.args();
    assert_eq!(args.len(), 2);
    assert_eq!(args[0].ty, Ty::BOOL);
    assert_eq!(args[1].ty, Ty::UNIT);
}

#[test]
fn body_args_empty_when_no_args() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let body = Body::dummy(owner);

    assert!(body.args().is_empty());
}

#[test]
fn body_basic_blocks_mutable() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut body = Body::dummy(owner);

    let bb1 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    assert_eq!(body.basic_blocks.len(), 2);
    match &body.basic_blocks[bb1].terminator.kind {
        TerminatorKind::Return => {}
        other => panic!("Expected Return, got {other:?}"),
    }
}

#[test]
fn dummy_with_different_owner() {
    let owner1 = DefId::new(CrateId::from_raw(5), LocalDefId::from_raw(10));
    let owner2 = DefId::new(CrateId::from_raw(99), LocalDefId::from_raw(200));

    let body1 = Body::dummy(owner1);
    let body2 = Body::dummy(owner2);

    assert_eq!(body1.owner.krate, CrateId::from_raw(5));
    assert_eq!(body1.owner.local_id, LocalDefId::from_raw(10));
    assert_eq!(body2.owner.krate, CrateId::from_raw(99));
    assert_eq!(body2.owner.local_id, LocalDefId::from_raw(200));
}

#[test]
fn dummy_locals_have_correct_mutability() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let body = Body::dummy(owner);

    assert_eq!(
        body.locals[LocalIdx::from_raw(0)].mutability,
        Mutability::Not
    );
}

#[test]
fn dummy_is_not_cleanup() {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let body = Body::dummy(owner);

    assert!(!body.basic_blocks[BasicBlockIdx::from_raw(0)].is_cleanup);
}
