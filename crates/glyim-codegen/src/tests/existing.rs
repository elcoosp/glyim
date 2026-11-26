use super::super::*;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use std::sync::Arc;

#[test]
fn test_ref_succeeds() {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    let local_idx = LocalIdx::from_usize(1);
    body.locals.push(glyim_mir::LocalDecl {
        ty: glyim_type::Ty::ERROR,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: glyim_mir::SourceInfo::new(glyim_span::Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_usize(0)]
        .statements
        .push(glyim_mir::Statement {
            kind: glyim_mir::StatementKind::Assign(
                glyim_mir::Place::new(local_idx),
                glyim_mir::Rvalue::Ref(
                    glyim_mir::Place::new(LocalIdx::from_usize(0)),
                    glyim_mir::BorrowKind::Shared,
                ),
            ),
            source_info: glyim_mir::SourceInfo::new(glyim_span::Span::DUMMY),
        });

    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok(), "Rvalue::Ref should succeed (implemented)");
}

#[test]
fn test_call_terminator_succeeds() {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    let dest = LocalIdx::from_usize(1);
    body.locals.push(LocalDecl {
        ty: glyim_type::Ty::ERROR,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_usize(0)].terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Int(0),
                ty: glyim_type::Ty::ERROR,
                span: glyim_span::Span::DUMMY,
            }),
            args: Vec::new(),
            destination: Place::new(dest),
            target: Some(BasicBlockIdx::from_usize(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(
        result.is_ok(),
        "Call terminator should succeed now that it's implemented"
    );
}

#[test]
fn test_projection_succeeds() {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    let local_idx = LocalIdx::from_usize(1);
    body.locals.push(glyim_mir::LocalDecl {
        ty: glyim_type::Ty::ERROR,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: glyim_mir::SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let place_with_proj = Place {
        local: local_idx,
        projection: Box::new([ProjectionElem::Deref]),
    };
    body.basic_blocks[BasicBlockIdx::from_usize(0)]
        .statements
        .push(glyim_mir::Statement {
            kind: glyim_mir::StatementKind::Assign(
                place_with_proj,
                glyim_mir::Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(1),
                    ty: glyim_type::Ty::ERROR,
                    span: glyim_span::Span::DUMMY,
                })),
            ),
            source_info: glyim_mir::SourceInfo::new(glyim_span::Span::DUMMY),
        });

    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(
        result.is_ok(),
        "Projection should succeed (handled via fallback)"
    );
}

#[test]
fn test_dummy_body_succeeds() {
    let body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok(), "Dummy body should compile successfully");
}
