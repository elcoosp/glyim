//! S17-T04: Allows disjoint partial moves of struct fields.

use crate::tests::test_ctx::TestBorrowckCtx;
use crate::{BorrowckResult, check_borrows};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, Operand, Place, ProjectionElem, Rvalue, SourceInfo, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{FieldIdx, GenericArg, Ty, TyKind};

#[test]
fn partial_move_allows_other_field_use() {
    let mut ctx_mut = test_ty_ctx();
    let elem_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(elem_ty), GenericArg::Ty(elem_ty)]);
    let tuple_ty = ctx_mut.mk_ty(TyKind::Tuple(subst));
    let ctx = ctx_mut.freeze();

    let mut body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: IndexVec::new(),
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    let local_tup = body.locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let local_field0 = body.locals.push(LocalDecl {
        ty: elem_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let local_field1 = body.locals.push(LocalDecl {
        ty: elem_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let field0_proj = ProjectionElem::Field(FieldIdx::from_raw(0));
    let field1_proj = ProjectionElem::Field(FieldIdx::from_raw(1));
    let place_field0 = Place {
        local: local_tup,
        projection: Box::new([field0_proj]),
    };
    let place_field1 = Place {
        local: local_tup,
        projection: Box::new([field1_proj]),
    };

    let move_field0 = Statement {
        kind: StatementKind::Assign(
            Place::new(local_field0),
            Rvalue::Use(Operand::Move(place_field0)),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let use_field1 = Statement {
        kind: StatementKind::Assign(
            Place::new(local_field1),
            Rvalue::Use(Operand::Copy(place_field1)),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![move_field0, use_field1],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    body.basic_blocks.push(block);

    let test_ctx = TestBorrowckCtx::new(&ctx, &body);
    let BorrowckResult { errors } = check_borrows(&test_ctx, &body);
    assert!(
        errors.is_empty(),
        "Expected no error from partial move of disjoint fields, got {errors:?}"
    );
}
