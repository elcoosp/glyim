//! Edge case tests for the borrow checker.

use crate::check_borrows;
use glyim_core::Mutability;
use glyim_mir::{BorrowKind, StatementKind};
use glyim_test::{assert_has_errors, assert_no_errors, with_fresh_ty_ctx};
use glyim_type::Region;

use super::mir_builder::{MirBodyBuilder, TestBorrowckCtx, assign_borrow, assign_copy, goto, ret};

/// Body with no borrows at all → no error.
#[test]
fn no_borrows_at_all_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(ret());
        b.push_stmt(bb0, assign_copy(local_2, local_1));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Body with a single basic block and a self-contained borrow → no error.
#[test]
fn single_block_borrow_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_bool, Mutability::Not);
        let local_3 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(ret());
        b.push_stmt(bb0, assign_borrow(local_2, local_1, BorrowKind::Shared));
        b.push_stmt(bb0, assign_copy(local_3, local_2));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Borrow that is never used — dest local is dead → no conflict even with
/// a second borrow of the same place.
///
///   BB0: `local_2` = &mut `local_1`; goto BB1    (`local_2` is never read)
///   BB1: `local_3` = &shared `local_1`; `local_4` = copy `local_3`; return
#[test]
fn dead_borrow_no_conflict() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);
        let ref_mut_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Mut);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_mut_bool, Mutability::Mut);
        let local_3 = b.add_local(ref_bool, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);

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

        let bb1 = b.push_block(ret());
        b.push_stmt(bb1, assign_borrow(local_3, local_1, BorrowKind::Shared));
        b.push_stmt(bb1, assign_copy(local_4, local_3));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Body with only a return — no statements at all.
#[test]
fn empty_body_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let mut b = MirBodyBuilder::new(bool_ty);
        b.push_block(ret());
        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Borrow of different places — should not conflict.
///
///   BB0: `local_2` = &mut `local_1`; `local_4` = &shared `local_3`; goto BB1
///   BB1: `local_5` = copy `local_2`; `local_6` = copy `local_4`; return
#[test]
fn borrows_of_different_places_no_conflict() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);
        let ref_mut_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Mut);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_mut_bool, Mutability::Mut);
        let local_3 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(ref_bool, Mutability::Not);
        let local_5 = b.add_local(bool_ty, Mutability::Not);
        let local_6 = b.add_local(bool_ty, Mutability::Not);

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
        b.push_stmt(bb0, assign_borrow(local_4, local_3, BorrowKind::Shared));

        let bb1 = b.push_block(ret());
        b.push_stmt(bb1, assign_copy(local_5, local_2));
        b.push_stmt(bb1, assign_copy(local_6, local_4));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Multiple shared borrows of the same place across different blocks → no error.
///
///   BB0: `local_2` = &shared `local_1`; goto BB1
///   BB1: `local_3` = &shared `local_1`; goto BB2
///   BB2: `local_4` = copy `local_2`; `local_5` = copy `local_3`; return
#[test]
fn multiple_shared_borrows_across_blocks_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_bool, Mutability::Not);
        let local_3 = b.add_local(ref_bool, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);
        let local_5 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(goto(1));
        b.push_stmt(bb0, assign_borrow(local_2, local_1, BorrowKind::Shared));

        let bb1 = b.push_block(goto(2));
        b.push_stmt(bb1, assign_borrow(local_3, local_1, BorrowKind::Shared));

        let bb2 = b.push_block(ret());
        b.push_stmt(bb2, assign_copy(local_4, local_2));
        b.push_stmt(bb2, assign_copy(local_5, local_3));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Body with StorageLive/StorageDead statements — should be ignored.
///
///   BB0: `StorageLive(local_1)`; `local_2` = &shared `local_1`; `local_3` = copy `local_2`; `StorageDead(local_1)`; return
#[test]
fn storage_live_dead_ignored_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_bool, Mutability::Not);
        let local_3 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(ret());
        b.push_stmt(bb0, StatementKind::StorageLive(local_1));
        b.push_stmt(bb0, assign_borrow(local_2, local_1, BorrowKind::Shared));
        b.push_stmt(bb0, assign_copy(local_3, local_2));
        b.push_stmt(bb0, StatementKind::StorageDead(local_1));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Unique borrow conflicts with shared borrow on same place → error.
///
///   BB0: `local_2` = &unique `local_1`; goto BB1
///   BB1: `local_3` = &shared `local_1`; `local_4` = copy `local_2`; return
#[test]
fn unique_and_shared_conflict_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);
        let ref_unique_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_unique_bool, Mutability::Not);
        let local_3 = b.add_local(ref_bool, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(goto(1));
        b.push_stmt(bb0, assign_borrow(local_2, local_1, BorrowKind::Unique));

        let bb1 = b.push_block(ret());
        b.push_stmt(bb1, assign_borrow(local_3, local_1, BorrowKind::Shared));
        b.push_stmt(bb1, assign_copy(local_4, local_2));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_has_errors(&result.errors);
}

/// Unique borrow expires, then shared borrow of same place → no error.
///
///   BB0: `local_2` = &unique `local_1`; `local_3` = copy `local_2`; goto BB1   (last use of `local_2`)
///   BB1: `local_4` = &shared `local_1`; `local_5` = copy `local_4`; return
#[test]
fn unique_expires_then_shared_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);
        let ref_unique_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_unique_bool, Mutability::Not);
        let local_3 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(ref_bool, Mutability::Not);
        let local_5 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(goto(1));
        b.push_stmt(bb0, assign_borrow(local_2, local_1, BorrowKind::Unique));
        b.push_stmt(bb0, assign_copy(local_3, local_2));

        let bb1 = b.push_block(ret());
        b.push_stmt(bb1, assign_borrow(local_4, local_1, BorrowKind::Shared));
        b.push_stmt(bb1, assign_copy(local_5, local_4));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}
