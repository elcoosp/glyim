//! S17-T03: Detects use-after-move errors.

use crate::tests::test_ctx::TestBorrowckCtx;
use crate::{BorrowckResult, check_borrows};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, Operand, Place, Rvalue, SourceInfo, Statement, StatementKind,
    Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Ty, TyKind};

#[test]
fn use_after_move_error() {
    let mut ctx_mut = test_ty_ctx();
    let subst = ctx_mut.intern_substitution(vec![]);
    let non_copy_ty = ctx_mut.mk_ty(TyKind::Adt(glyim_core::AdtId::from_raw(1), subst));
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

    let local_val = body.locals.push(LocalDecl {
        ty: non_copy_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let local_dest = body.locals.push(LocalDecl {
        ty: non_copy_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let move_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(local_dest),
            Rvalue::Use(Operand::Move(Place::new(local_val))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let use_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(local_dest),
            Rvalue::Use(Operand::Copy(Place::new(local_val))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![move_stmt, use_stmt],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    body.basic_blocks.push(block);

    let test_ctx = TestBorrowckCtx::new(&ctx, &body);
    let BorrowckResult { errors } = check_borrows(&test_ctx, &body);
    assert!(!errors.is_empty(), "Expected use-after-move error");
    let error_msg = errors[0].to_string();
    assert!(
        error_msg.contains("use of moved value"),
        "Error message did not mention use of moved value: {error_msg}"
    );
}
