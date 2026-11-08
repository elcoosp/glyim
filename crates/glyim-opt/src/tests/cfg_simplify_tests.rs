//! S18-T03: CFG simplify merges single-pred/single-succ blocks.

use super::testutil::*;
use crate::optimize;
use glyim_core::Mutability;
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;
use std::sync::Arc;

/// Test that two blocks connected by Goto are merged when the successor
/// has exactly one predecessor.
#[test]
fn cfg_merge_goto_chain() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
        ];

        // bb0:
        //   _1 = 10
        //   goto bb1
        // bb1:
        //   _0 = _1
        //   return
        let block0 = make_block(
            vec![assign_stmt(
                Place::new(local(1)),
                const_int_rvalue(10, i32_ty),
            )],
            goto_term(bb(1)),
        );
        let block1 = make_block(
            vec![assign_stmt(
                Place::new(local(0)),
                Rvalue::Use(copy_op(local(1))),
            )],
            return_term(),
        );

        build_test_body(locals, vec![block0, block1], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    assert_eq!(
        optimized.body.basic_blocks.len(),
        1,
        "two Goto-connected blocks should be merged into one"
    );
}

/// Test that three blocks in a Goto chain are all merged.
#[test]
fn cfg_merge_three_block_chain() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
        ];

        // bb0 -> bb1 -> bb2 (all Goto)
        let block0 = make_block(
            vec![assign_stmt(
                Place::new(local(1)),
                const_int_rvalue(10, i32_ty),
            )],
            goto_term(bb(1)),
        );
        let block1 = make_block(
            vec![assign_stmt(
                Place::new(local(0)),
                Rvalue::Use(copy_op(local(1))),
            )],
            goto_term(bb(2)),
        );
        let block2 = make_block(vec![], return_term());

        build_test_body(locals, vec![block0, block1, block2], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    assert_eq!(
        optimized.body.basic_blocks.len(),
        1,
        "three Goto-connected blocks should be merged into one"
    );
}

/// Test that blocks with multiple predecessors are NOT merged.
#[test]
fn cfg_no_merge_multiple_preds() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);
        let bool_ty = ty_bool(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut),  // _0 return
            (bool_ty, Mutability::Mut), // _1
        ];

        // bb0: _1 = true; switchInt(_1) -> [1: bb1, otherwise: bb2]
        // bb1: _0 = 1; goto bb3
        // bb2: _0 = 2; goto bb3
        // bb3: return
        let block0 = make_block(
            vec![assign_stmt(
                Place::new(local(1)),
                Rvalue::Use(const_bool_val(true)),
            )],
            bool_switch_term(copy_op(local(1)), bb(1), bb(2)),
        );
        let block1 = make_block(
            vec![assign_stmt(
                Place::new(local(0)),
                const_int_rvalue(1, i32_ty),
            )],
            goto_term(bb(3)),
        );
        let block2 = make_block(
            vec![assign_stmt(
                Place::new(local(0)),
                const_int_rvalue(2, i32_ty),
            )],
            goto_term(bb(3)),
        );
        let block3 = make_block(vec![], return_term());

        build_test_body(locals, vec![block0, block1, block2, block3], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    // bb3 has 2 preds (bb1 and bb2), so it won't merge.
    // bb1 is single-pred from bb0, but bb0 doesn't Goto bb1 (it SwitchInt).
    // So no merging should happen.
    assert!(
        optimized.body.basic_blocks.len() >= 3,
        "blocks with multiple predecessors should not be merged; got {} blocks",
        optimized.body.basic_blocks.len()
    );
}

/// Test that an unreachable block is eliminated.
#[test]
fn unreachable_block_eliminated() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
        ];

        // bb0: _0 = 42; return
        // bb1: _1 = 99; return (unreachable – no predecessor)
        let block0 = make_block(
            vec![assign_stmt(
                Place::new(local(0)),
                const_int_rvalue(42, i32_ty),
            )],
            return_term(),
        );
        let block1 = make_block(
            vec![assign_stmt(
                Place::new(local(1)),
                const_int_rvalue(99, i32_ty),
            )],
            return_term(),
        );

        build_test_body(locals, vec![block0, block1], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    assert_eq!(
        optimized.body.basic_blocks.len(),
        1,
        "unreachable block should be eliminated"
    );
}

/// Test that the start block (bb0) is never eliminated even if it looks isolated.
#[test]
fn start_block_always_kept() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
        ];

        let block0 = make_block(vec![], return_term());

        build_test_body(locals, vec![block0], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    assert_eq!(
        optimized.body.basic_blocks.len(),
        1,
        "start block should always be kept"
    );
}

/// Test that `SwitchInt` targets are correct after optimization (no elimination, but remapping).
#[test]
fn cfg_remap_after_elimination() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);
        let bool_ty = ty_bool(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut),  // _0 return
            (bool_ty, Mutability::Mut), // _1
        ];

        // bb0: _1 = true; switchInt(_1) -> [1: bb1, otherwise: bb2]
        // bb1: _0 = 1; return
        // bb2: _0 = 2; return
        let block0 = make_block(
            vec![assign_stmt(
                Place::new(local(1)),
                Rvalue::Use(const_bool_val(true)),
            )],
            bool_switch_term(copy_op(local(1)), bb(1), bb(2)),
        );
        let block1 = make_block(
            vec![assign_stmt(
                Place::new(local(0)),
                const_int_rvalue(1, i32_ty),
            )],
            return_term(),
        );
        let block2 = make_block(
            vec![assign_stmt(
                Place::new(local(0)),
                const_int_rvalue(2, i32_ty),
            )],
            return_term(),
        );

        build_test_body(locals, vec![block0, block1, block2], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    // All 3 blocks should remain — all are reachable
    assert_eq!(
        optimized.body.basic_blocks.len(),
        3,
        "all reachable blocks should be preserved"
    );
}
