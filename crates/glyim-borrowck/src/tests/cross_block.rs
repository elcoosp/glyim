//! Tests for borrows that cross basic block boundaries (V08-T01, T02, T03).

use crate::check_borrows;
use glyim_core::Mutability;
use glyim_mir::BorrowKind;
use glyim_test::{assert_no_errors, with_fresh_ty_ctx};
use glyim_type::Region;

use super::mir_builder::{
    MirBodyBuilder, TestBorrowckCtx, assign_borrow, assign_copy, goto, if_switch, ret,
};

/// V08-T01: Borrow that crosses a basic block (if condition) → no error.
///
/// A shared borrow is created in BB0, then used in both branches of an
/// if-else (BB1 then-branch, BB2 else-branch). No conflicting access
/// exists on any path, so this should produce no borrow errors.
///
/// MIR:
///   BB0: `local_2` = &shared `local_1`; switch `local_3` -> [true: BB1, false: BB2]
///   BB1: `local_4` = copy `local_2`; goto BB3
///   BB2: `local_5` = copy `local_2`; goto BB3
///   BB3: return
#[test]
fn borrow_crosses_basic_block_if_condition_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_bool, Mutability::Not);
        let local_3 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);
        let local_5 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(if_switch(local_3, bool_ty, 1, 2));
        b.push_stmt(bb0, assign_borrow(local_2, local_1, BorrowKind::Shared));

        let bb1 = b.push_block(goto(3));
        b.push_stmt(bb1, assign_copy(local_4, local_2));

        let bb2 = b.push_block(goto(3));
        b.push_stmt(bb2, assign_copy(local_5, local_2));

        b.push_block(ret());

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// V08-T02: Mutable borrow of field used after loop → allowed.
///
/// MIR:
///   BB0: `local_2` = &mut `local_1`; goto BB1
///   BB1: switch `local_3` -> [true: BB2, false: BB3]
///   BB2: `local_4` = copy `local_2`; goto BB1       (loop body)
///   BB3: `local_5` = copy `local_2`; return         (after loop)
#[test]
fn mutable_borrow_used_after_loop_allowed() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_mut_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Mut);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_mut_bool, Mutability::Mut);
        let local_3 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);
        let local_5 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(goto(1));
        b.push_stmt(
            bb0,
            assign_borrow(
                local_2,
                local_1,
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
        );

        let _bb1 = b.push_block(if_switch(local_3, bool_ty, 2, 3));

        let bb2 = b.push_block(goto(1));
        b.push_stmt(bb2, assign_copy(local_4, local_2));

        let bb3 = b.push_block(ret());
        b.push_stmt(bb3, assign_copy(local_5, local_2));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// V08-T03: Borrow expires after last use in different block → no error.
///
/// MIR:
///   BB0: `local_2` = &shared `local_1`; goto BB1
///   BB1: `local_3` = copy `local_2`; goto BB2        (last use of `local_2`)
///   BB2: `local_4` = copy `local_1`; return          (borrow expired, direct access OK)
#[test]
fn borrow_expires_after_last_use_different_block_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_bool, Mutability::Not);
        let local_3 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(goto(1));
        b.push_stmt(bb0, assign_borrow(local_2, local_1, BorrowKind::Shared));

        let bb1 = b.push_block(goto(2));
        b.push_stmt(bb1, assign_copy(local_3, local_2));

        let bb2 = b.push_block(ret());
        b.push_stmt(bb2, assign_copy(local_4, local_1));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}
