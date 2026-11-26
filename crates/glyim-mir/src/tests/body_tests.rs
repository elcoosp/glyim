use crate::*;

#[test]
fn body_dummy_creates_valid_empty_body() {
    let owner = glyim_core::def_id::DefId::new(
        glyim_core::def_id::CrateId::from_usize(42),
        glyim_core::def_id::LocalDefId::from_usize(123),
    );
    let body = Body::dummy(owner);
    assert_eq!(body.owner, owner);
    assert_eq!(body.basic_blocks.len(), 1, "Should have one basic block");
    let terminator = &body.basic_blocks[BasicBlockIdx::from_usize(0)].terminator;
    match terminator.kind {
        TerminatorKind::Unreachable => {}
        _ => panic!(
            "Terminator should be Unreachable, got {:?}",
            terminator.kind
        ),
    }
    assert_eq!(body.locals.len(), 1, "Should have one local");
    assert_eq!(body.locals[LocalIdx::from_usize(0)].ty, Ty::UNIT);
    assert_eq!(body.arg_count, 0);
    assert_eq!(body.return_ty, Ty::UNIT);
    assert_eq!(body.span, Span::DUMMY);
    assert!(body.var_debug_info.is_empty());
}
