//! S17-T02: Allows shared borrows in two-phase reservation.

use crate::tests::test_ctx::TestBorrowckCtx;
use crate::{BorrowckResult, check_borrows};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_mir::{
    BasicBlockData, Body, BorrowKind, LocalDecl, Place, Rvalue, SourceInfo, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Region, Ty, TyKind};

#[test]
fn two_phase_reservation_allows_shared_borrow() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
    let ref_mut_ty = ctx_mut.mk_ref(Region::Erased, i32_ty, Mutability::Mut);
    let ref_shared_ty = ctx_mut.mk_ref(Region::Erased, i32_ty, Mutability::Not);
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

    let local_x = body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let local_ref_mut = body.locals.push(LocalDecl {
        ty: ref_mut_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let local_ref_shared = body.locals.push(LocalDecl {
        ty: ref_shared_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place_x = Place::new(local_x);
    let two_phase_borrow = Statement {
        kind: StatementKind::Assign(
            Place::new(local_ref_mut),
            Rvalue::Ref(
                place_x.clone(),
                BorrowKind::Mut {
                    allow_two_phase_borrow: true,
                },
            ),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let shared_borrow = Statement {
        kind: StatementKind::Assign(
            Place::new(local_ref_shared),
            Rvalue::Ref(place_x, BorrowKind::Shared),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![two_phase_borrow, shared_borrow],
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
        "Expected no error from shared borrow during two-phase reservation, got {errors:?}"
    );
}
