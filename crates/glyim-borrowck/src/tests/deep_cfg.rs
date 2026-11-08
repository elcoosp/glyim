//! Tests for deeply nested control flow with borrows.

use crate::check_borrows;
use glyim_core::Mutability;
use glyim_mir::BorrowKind;
use glyim_test::{assert_has_errors, assert_no_errors, with_fresh_ty_ctx};
use glyim_type::Region;

use super::mir_builder::{
    MirBodyBuilder, TestBorrowckCtx, assign_borrow, assign_copy, goto, if_switch, ret,
};

/// Nested if: shared borrow in outer scope, used in inner branches → no error.
///
///   BB0: `local_2` = &shared `local_1`; goto BB1
///   BB1: switch `local_3` -> [true: BB2, false: BB5]
///   BB2: switch `local_4` -> [true: BB3, false: BB4]
///   BB3: `local_5` = copy `local_2`; goto BB6
///   BB4: `local_6` = copy `local_2`; goto BB6
///   BB5: `local_7` = copy `local_2`; goto BB6
///   BB6: return
#[test]
fn nested_if_shared_borrow_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_bool, Mutability::Not);
        let local_3 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);
        let local_5 = b.add_local(bool_ty, Mutability::Not);
        let local_6 = b.add_local(bool_ty, Mutability::Not);
        let local_7 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(goto(1));
        b.push_stmt(bb0, assign_borrow(local_2, local_1, BorrowKind::Shared));

        b.push_block(if_switch(local_3, bool_ty, 2, 5));

        b.push_block(if_switch(local_4, bool_ty, 3, 4));

        let bb3 = b.push_block(goto(6));
        b.push_stmt(bb3, assign_copy(local_5, local_2));

        let bb4 = b.push_block(goto(6));
        b.push_stmt(bb4, assign_copy(local_6, local_2));

        let bb5 = b.push_block(goto(6));
        b.push_stmt(bb5, assign_copy(local_7, local_2));

        b.push_block(ret());

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Nested if: mut borrow in outer scope, conflicting access in branch where
/// the mut ref is also used after the branch (so it's live) → error.
///
///   BB0: `local_2` = &mut `local_1`; goto BB1
///   BB1: switch `local_3` -> [true: BB2, false: BB4]
///   BB2: switch `local_4` -> [true: BB3, false: BB6]
///   BB3: `local_5` = copy `local_2`; goto BB5          (use mut ref, OK)
///   BB4: `local_6` = copy `local_1`; goto BB5          (direct read while mut borrowed, ERROR)
///   BB5: `local_7` = copy `local_2`; return            (use mut ref — makes `local_2` live on ALL paths into BB5)
///   BB6: `local_8` = copy `local_2`; goto BB5          (use mut ref, OK)
#[test]
fn nested_if_mut_borrow_conflict_in_branch_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_mut_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Mut);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_mut_bool, Mutability::Mut);
        let local_3 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);
        let local_5 = b.add_local(bool_ty, Mutability::Not);
        let local_6 = b.add_local(bool_ty, Mutability::Not);
        let local_7 = b.add_local(bool_ty, Mutability::Not);
        let local_8 = b.add_local(bool_ty, Mutability::Not);

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

        b.push_block(if_switch(local_3, bool_ty, 2, 4));

        b.push_block(if_switch(local_4, bool_ty, 3, 6));

        let bb3 = b.push_block(goto(5));
        b.push_stmt(bb3, assign_copy(local_5, local_2));

        // BB4: conflicting direct read of local_1 while local_2 is live
        let bb4 = b.push_block(goto(5));
        b.push_stmt(bb4, assign_copy(local_6, local_1));

        // BB5: use of local_2 — this makes local_2 live at BB4 as well
        let bb5 = b.push_block(ret());
        b.push_stmt(bb5, assign_copy(local_7, local_2));

        let bb6 = b.push_block(goto(5));
        b.push_stmt(bb6, assign_copy(local_8, local_2));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_has_errors(&result.errors);
}

/// Nested if: mut borrow only used on one branch, conflicting access on
/// another branch where the ref is dead → no error (correct NLL).
///
///   BB0: `local_2` = &mut `local_1`; goto BB1
///   BB1: switch `local_3` -> [true: BB2, false: BB3]
///   BB2: `local_4` = copy `local_2`; goto BB4          (use mut ref, then merge)
///   BB3: `local_5` = copy `local_1`; goto BB4          (mut ref dead on this path, OK)
///   BB4: return                           (`local_2` not used here, so dead on BB3 path)
#[test]
fn nested_if_mut_borrow_dead_on_other_branch_no_error() {
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

        b.push_block(if_switch(local_3, bool_ty, 2, 3));

        let bb2 = b.push_block(goto(4));
        b.push_stmt(bb2, assign_copy(local_4, local_2));

        // BB3: direct read of local_1 — local_2 is NOT live here (never used after BB3)
        let bb3 = b.push_block(goto(4));
        b.push_stmt(bb3, assign_copy(local_5, local_1));

        b.push_block(ret());

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Diamond CFG: shared borrow created before branch, used after merge → no error.
///
///   BB0: `local_2` = &shared `local_1`; goto BB1
///   BB1: switch `local_3` -> [true: BB2, false: BB3]
///   BB2: goto BB4
///   BB3: goto BB4
///   BB4: `local_4` = copy `local_2`; return
#[test]
fn diamond_cfg_shared_borrow_no_error() {
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

        b.push_block(if_switch(local_3, bool_ty, 2, 3));

        b.push_block(goto(4));
        b.push_block(goto(4));

        let bb4 = b.push_block(ret());
        b.push_stmt(bb4, assign_copy(local_4, local_2));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Shared borrow in one branch of diamond, used after merge.
/// The borrow only exists on one path, but the ref local may be used after.
///
///   BB0: switch `local_3` -> [true: BB1, false: BB2]
///   BB1: `local_2` = &shared `local_1`; goto BB3
///   BB2: goto BB3
///   BB3: `local_4` = copy `local_2`; return
///
/// No borrow conflict — the borrow of `local_1` only exists on the BB1 path,
/// and on the BB2 path there is no active loan to conflict with.
#[test]
fn borrow_in_one_branch_of_diamond_no_conflict() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_bool, Mutability::Not);
        let local_3 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);

        b.push_block(if_switch(local_3, bool_ty, 1, 2));

        let bb1 = b.push_block(goto(3));
        b.push_stmt(bb1, assign_borrow(local_2, local_1, BorrowKind::Shared));

        b.push_block(goto(3));

        let bb3 = b.push_block(ret());
        b.push_stmt(bb3, assign_copy(local_4, local_2));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}
