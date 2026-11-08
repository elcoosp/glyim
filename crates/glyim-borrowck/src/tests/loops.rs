//! Tests for borrows in loop contexts.

use crate::check_borrows;
use glyim_core::Mutability;
use glyim_mir::BorrowKind;
use glyim_test::{assert_has_errors, assert_no_errors, with_fresh_ty_ctx};
use glyim_type::Region;

use super::mir_builder::{
    MirBodyBuilder, TestBorrowckCtx, assign_borrow, assign_copy, goto, if_switch, ret,
};

/// Shared borrow inside loop body, re-borrowed each iteration → no error.
///
///   BB0: goto BB1
///   BB1: switch `local_2` -> [true: BB2, false: BB3]
///   BB2: `local_4` = &shared `local_1`; `local_5` = copy `local_4`; goto BB1
///   BB3: return
#[test]
fn shared_borrow_inside_loop_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(ref_bool, Mutability::Not);
        let local_5 = b.add_local(bool_ty, Mutability::Not);

        b.push_block(goto(1));

        b.push_block(if_switch(local_2, bool_ty, 2, 3));

        let bb2 = b.push_block(goto(1));
        b.push_stmt(bb2, assign_borrow(local_4, local_1, BorrowKind::Shared));
        b.push_stmt(bb2, assign_copy(local_5, local_4));

        b.push_block(ret());

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Mutable borrow inside loop that conflicts with post-loop access → error.
///
///   BB0: goto BB1
///   BB1: switch `local_2` -> [true: BB2, false: BB3]
///   BB2: `local_4` = &mut `local_1`; `local_5` = copy `local_4`; goto BB1
///   BB3: `local_6` = copy `local_1`; return
///
/// `local_4` is a fresh mut borrow each iteration, but it dies at the end of BB2.
#[test]
fn mut_borrow_inside_loop_then_access_after_ok() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_mut_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Mut);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(ref_mut_bool, Mutability::Mut);
        let local_5 = b.add_local(bool_ty, Mutability::Not);
        let local_6 = b.add_local(bool_ty, Mutability::Not);

        b.push_block(goto(1));

        b.push_block(if_switch(local_2, bool_ty, 2, 3));

        let bb2 = b.push_block(goto(1));
        b.push_stmt(
            bb2,
            assign_borrow(
                local_4,
                local_1,
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
        );
        b.push_stmt(bb2, assign_copy(local_5, local_4));

        let bb3 = b.push_block(ret());
        b.push_stmt(bb3, assign_copy(local_6, local_1));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}

/// Mut borrow before loop, accessed inside AND after loop → no error.
/// This tests that the borrow stays live across loop back-edges.
///
///   BB0: `local_2` = &mut `local_1`; goto BB1
///   BB1: switch `local_3` -> [true: BB2, false: BB3]
///   BB2: `local_4` = copy `local_2`; goto BB1
///   BB3: `local_5` = copy `local_2`; return
#[test]
fn mut_borrow_before_loop_used_throughout_no_error() {
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

/// Mut borrow before loop, conflicting access inside loop → error.
///
///   BB0: `local_2` = &mut `local_1`; goto BB1
///   BB1: switch `local_3` -> [true: BB2, false: BB4]
///   BB2: `local_4` = copy `local_1`; goto BB3    (direct read while mut borrowed!)
///   BB3: `local_5` = copy `local_2`; goto BB1
///   BB4: `local_6` = copy `local_2`; return
#[test]
fn mut_borrow_before_loop_conflicting_access_in_loop_error() {
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

        // BB2: conflicting direct read of local_1
        let bb2 = b.push_block(goto(3));
        b.push_stmt(bb2, assign_copy(local_4, local_1));

        // BB3: use of local_2 (keeps it live)
        let bb3 = b.push_block(goto(1));
        b.push_stmt(bb3, assign_copy(local_5, local_2));

        let bb4 = b.push_block(ret());
        b.push_stmt(bb4, assign_copy(local_6, local_2));

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_has_errors(&result.errors);
}

/// Double loop (nested): shared borrow in outer, used in inner → no error.
///
///   BB0: `local_2` = &shared `local_1`; goto BB1
///   BB1: switch `local_3` -> [true: BB2, false: BB5]   (outer loop header)
///   BB2: switch `local_4` -> [true: BB3, false: BB4]   (inner loop header)
///   BB3: `local_5` = copy `local_2`; goto BB2                  (inner loop body)
///   BB4: goto BB1                                 (inner loop exit, back to outer)
///   BB5: return
#[test]
fn nested_loops_shared_borrow_no_error() {
    let (ty_ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let bool_ty = ctx_mut.bool_ty();
        let ref_bool = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

        let mut b = MirBodyBuilder::new(bool_ty);
        let local_1 = b.add_local(bool_ty, Mutability::Not);
        let local_2 = b.add_local(ref_bool, Mutability::Not);
        let local_3 = b.add_local(bool_ty, Mutability::Not);
        let local_4 = b.add_local(bool_ty, Mutability::Not);
        let local_5 = b.add_local(bool_ty, Mutability::Not);

        let bb0 = b.push_block(goto(1));
        b.push_stmt(bb0, assign_borrow(local_2, local_1, BorrowKind::Shared));

        b.push_block(if_switch(local_3, bool_ty, 2, 5));

        b.push_block(if_switch(local_4, bool_ty, 3, 4));

        let bb3 = b.push_block(goto(2));
        b.push_stmt(bb3, assign_copy(local_5, local_2));

        b.push_block(goto(1));

        b.push_block(ret());

        b.build()
    });

    let mock_ctx = TestBorrowckCtx::new(&ty_ctx, &body);
    let result = check_borrows(&mock_ctx, &body);
    assert_no_errors(&result.errors);
}
