//! S17-T01: Detects double mutable borrow error.

use crate::tests::test_ctx::TestBorrowckCtx;
use crate::{BorrowckResult, check_borrows};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_mir::{
    BasicBlockData, Body, BorrowKind, LocalDecl, Operand, Place, Rvalue, SourceInfo, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Region, Ty, TyKind};

#[test]
fn double_mutable_borrow_error() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
    let ref_ty = ctx_mut.mk_ref(Region::Erased, i32_ty, Mutability::Mut);
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

    let local_ref1 = body.locals.push(LocalDecl {
        ty: ref_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let local_ref2 = body.locals.push(LocalDecl {
        ty: ref_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    // Dummy locals to hold reads of the references
    let dummy1 = body.locals.push(LocalDecl {
        ty: ref_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let dummy2 = body.locals.push(LocalDecl {
        ty: ref_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place_x = Place::new(local_x);
    let borrow_stmt1 = Statement {
        kind: StatementKind::Assign(
            Place::new(local_ref1),
            Rvalue::Ref(
                place_x.clone(),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let borrow_stmt2 = Statement {
        kind: StatementKind::Assign(
            Place::new(local_ref2),
            Rvalue::Ref(
                place_x,
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    // Use both references after the second borrow to keep them live
    let use_ref1 = Statement {
        kind: StatementKind::Assign(
            Place::new(dummy1),
            Rvalue::Use(Operand::Copy(Place::new(local_ref1))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let use_ref2 = Statement {
        kind: StatementKind::Assign(
            Place::new(dummy2),
            Rvalue::Use(Operand::Copy(Place::new(local_ref2))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![borrow_stmt1, borrow_stmt2, use_ref1, use_ref2],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    body.basic_blocks.push(block);

    let test_ctx = TestBorrowckCtx::new(&ctx, &body);
    let BorrowckResult { errors } = check_borrows(&test_ctx, &body);
    assert!(!errors.is_empty(), "Expected a borrow error");
    let error_msg = errors[0].to_string();
    assert!(
        error_msg.contains("cannot borrow") && error_msg.contains("mutable"),
        "Error message did not mention mutable borrow conflict: {error_msg}"
    );
}
