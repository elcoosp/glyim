//! Tests for field projection layout (S08-T01)

use crate::{BytecodeBackend, CodegenBackend};
use glyim_core::primitives::Mutability;
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, LocalIdx, Place, ProjectionElem, Rvalue, SourceInfo,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{FieldIdx, TyKind};
use std::sync::Arc;

#[test]
fn field_projection_emits_offset_bytecode() {
    let (_, body) = with_fresh_ty_ctx(|ctx| {
        let unit_ty = ctx.unit_ty();
        let ptr_ty = ctx.mk_ty(TyKind::RawPtr(unit_ty, Mutability::Not));

        let mut body = Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ));

        body.locals.push(LocalDecl {
            ty: ptr_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        body.locals.push(LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let place = Place {
            local: LocalIdx::from_usize(0),
            projection: vec![ProjectionElem::Field(FieldIdx::from_usize(1))].into_boxed_slice(),
        };

        let stmt = Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_usize(1)),
                Rvalue::Ref(place, glyim_mir::BorrowKind::Shared),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };

        let mut block = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        block.statements.push(stmt);
        body.basic_blocks.push(block);

        body
    });

    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));

    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(!bc.is_empty());
}
