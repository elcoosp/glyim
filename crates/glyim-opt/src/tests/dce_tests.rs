//! S18-T02: DCE removes dead stores.

use super::testutil::*;
use crate::optimize;
use glyim_core::Mutability;
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use std::sync::Arc;

/// Test that an assignment to a local that is never read is removed.
#[test]
fn dce_removes_dead_store() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
            (i32_ty, Mutability::Mut), // _2 (dead)
        ];

        // bb0:
        //   _2 = 42          // dead — never read
        //   _1 = 10
        //   _0 = _1
        //   return
        let block0 = make_block(
            vec![
                assign_stmt(Place::new(local(2)), const_int_rvalue(42, i32_ty)),
                assign_stmt(Place::new(local(1)), const_int_rvalue(10, i32_ty)),
                assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(1)))),
            ],
            return_term(),
        );

        build_test_body(locals, vec![block0], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    let block = &optimized.body.basic_blocks[bb(0)];

    // The dead store _2 = 42 should be removed.
    for stmt in &block.statements {
        if let StatementKind::Assign(place, _) = &stmt.kind {
            assert_ne!(
                place.local,
                local(2),
                "dead store to _2 should be eliminated"
            );
        }
    }
}

/// Test that a used assignment is NOT removed.
#[test]
fn dce_keeps_used_store() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
        ];

        // bb0:
        //   _1 = 10
        //   _0 = _1          // _1 is used here
        //   return
        let block0 = make_block(
            vec![
                assign_stmt(Place::new(local(1)), const_int_rvalue(10, i32_ty)),
                assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(1)))),
            ],
            return_term(),
        );

        build_test_body(locals, vec![block0], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    let block = &optimized.body.basic_blocks[bb(0)];

    // After const prop: _0 = 10, _1 = 10. Then DCE: _1 = 10 is dead (not used).
    // _0 = 10 must remain.
    let has_return_assign = block.statements.iter().any(|stmt| {
        if let StatementKind::Assign(place, _) = &stmt.kind {
            place.local == local(0)
        } else {
            false
        }
    });
    assert!(has_return_assign, "_0 should still have an assignment");
}

/// Test that multiple dead stores are all removed.
#[test]
fn dce_removes_multiple_dead_stores() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
            (i32_ty, Mutability::Mut), // _2 (dead)
            (i32_ty, Mutability::Mut), // _3 (dead)
        ];

        // bb0:
        //   _2 = 1           // dead
        //   _3 = 2           // dead
        //   _1 = 10
        //   _0 = _1
        //   return
        let block0 = make_block(
            vec![
                assign_stmt(Place::new(local(2)), const_int_rvalue(1, i32_ty)),
                assign_stmt(Place::new(local(3)), const_int_rvalue(2, i32_ty)),
                assign_stmt(Place::new(local(1)), const_int_rvalue(10, i32_ty)),
                assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(1)))),
            ],
            return_term(),
        );

        build_test_body(locals, vec![block0], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    let block = &optimized.body.basic_blocks[bb(0)];

    for stmt in &block.statements {
        if let StatementKind::Assign(place, _) = &stmt.kind {
            assert!(
                place.local != local(2) && place.local != local(3),
                "dead stores to _2 and _3 should be eliminated"
            );
        }
    }
}

/// Test that a local used in a terminator operand is NOT dead.
#[test]
fn dce_keeps_store_used_in_terminator() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);
        let bool_ty = ty_bool(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut),  // _0 return
            (bool_ty, Mutability::Mut), // _1
        ];

        // bb0:
        //   _1 = true
        //   switchInt(_1) -> [1: bb1, otherwise: bb2]
        // bb1:
        //   _0 = 1
        //   return
        // bb2:
        //   _0 = 2
        //   return
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

    // _1 = true should NOT be removed — it's used in the SwitchInt terminator.
    let block0 = &optimized.body.basic_blocks[bb(0)];
    let has_assign_to_1 = block0.statements.iter().any(|stmt| {
        if let StatementKind::Assign(place, _) = &stmt.kind {
            place.local == local(1)
        } else {
            false
        }
    });
    assert!(
        has_assign_to_1,
        "_1 assignment should be kept because it's used in terminator"
    );
}

/// Test that StorageLive/StorageDead statements are preserved.
#[test]
fn dce_preserves_storage_statements() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
        ];

        // bb0:
        //   StorageLive(_1)
        //   _1 = 10
        //   _0 = _1
        //   StorageDead(_1)
        //   return
        let block0 = make_block(
            vec![
                storage_live_stmt(local(1)),
                assign_stmt(Place::new(local(1)), const_int_rvalue(10, i32_ty)),
                assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(1)))),
                storage_dead_stmt(local(1)),
            ],
            return_term(),
        );

        build_test_body(locals, vec![block0], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    let block = &optimized.body.basic_blocks[bb(0)];

    let has_storage_live = block
        .statements
        .iter()
        .any(|stmt| matches!(&stmt.kind, StatementKind::StorageLive(l) if *l == local(1)));
    let has_storage_dead = block
        .statements
        .iter()
        .any(|stmt| matches!(&stmt.kind, StatementKind::StorageDead(l) if *l == local(1)));
    assert!(has_storage_live, "StorageLive should be preserved");
    assert!(has_storage_dead, "StorageDead should be preserved");
}

/// MIR-6 regression: DCE must treat a `Drop { place }` terminator as a *use*
/// of `place.local`. Lowering emits scope-exit `Drop` terminators; without
/// this, DCE deleted the only store feeding the dropped local, so the drop
/// ran on uninitialized memory.
#[test]
fn dce_keeps_store_used_by_drop_terminator() {
    let (ctx, mut body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);
        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1 dropped
        ];
        // bb0:
        //   _1 = 42          (the only store feeding the drop)
        //   Drop(_1) -> bb1
        // bb1:
        //   return
        //
        // NOTE: this calls `dce::run` directly rather than `optimize`, because
        // `optimize` runs a validator that rejects a `Drop` on a non-droppable
        // type (`i32`). The DCE pass itself is the unit under test and must
        // keep the store regardless of the dropped type.
        let block0 = make_block(
            vec![assign_stmt(Place::new(local(1)), const_int_rvalue(42, i32_ty))],
            Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(local(1)),
                    target: bb(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
        );
        let block1 = make_block(vec![], return_term());
        build_test_body(locals, vec![block0, block1], 0, i32_ty)
    });

    crate::dce::run(&ctx, &mut body);
    let block = &body.basic_blocks[bb(0)];

    let kept = block.statements.iter().any(|stmt| {
        matches!(&stmt.kind, StatementKind::Assign(p, _) if p.local == local(1))
    });
    assert!(
        kept,
        "the store feeding `Drop(_1)` must survive DCE"
    );
}
